package api

import (
	"bufio"
	"fmt"
	"io"
	"log"
	"os"
	"os/exec"
	"sort"
)

// RunHook executes the given path as a subprocess in a fire-and-forget manner.
//
// The path is exec'd directly with no arguments and no shell, so the caller
// controls exactly what runs. The parent process's environment is inherited,
// with extraEnv layered on top (later entries win). stdout and stderr are
// captured and relayed to the standard logger, one line at a time, prefixed
// with the hook event (taken from extraEnv["USQUE_EVENT"] when present).
//
// If path is empty, RunHook is a no-op. The function returns immediately; the
// subprocess and its log relays run in background goroutines.
//
// Parameters:
//   - path:     string            - Absolute or $PATH-resolvable executable to run.
//   - extraEnv: map[string]string - Additional environment variables to expose.
func RunHook(path string, extraEnv map[string]string) {
	if path == "" {
		return
	}

	event := extraEnv["USQUE_EVENT"]
	if event == "" {
		event = "hook"
	}
	prefix := fmt.Sprintf("hook[%s]", event)

	go func() {
		cmd := exec.Command(path)
		cmd.Env = mergeEnv(os.Environ(), extraEnv)

		stdout, err := cmd.StdoutPipe()
		if err != nil {
			log.Printf("%s: failed to create stdout pipe: %v", prefix, err)
			return
		}
		stderr, err := cmd.StderrPipe()
		if err != nil {
			log.Printf("%s: failed to create stderr pipe: %v", prefix, err)
			return
		}

		if err := cmd.Start(); err != nil {
			log.Printf("%s: failed to start %q: %v", prefix, path, err)
			return
		}

		done := make(chan struct{}, 2)
		go relayHookOutput(stdout, prefix+" stdout", done)
		go relayHookOutput(stderr, prefix+" stderr", done)
		<-done
		<-done

		if err := cmd.Wait(); err != nil {
			log.Printf("%s: %q exited with error: %v", prefix, path, err)
		}
	}()
}

// relayHookOutput reads r line-by-line and logs each line prefixed with label.
// It signals completion on done exactly once.
func relayHookOutput(r io.Reader, label string, done chan<- struct{}) {
	defer func() { done <- struct{}{} }()
	scanner := bufio.NewScanner(r)
	scanner.Buffer(make([]byte, 64*1024), 1024*1024)
	for scanner.Scan() {
		log.Printf("%s: %s", label, scanner.Text())
	}
}

// mergeEnv returns a new slice containing base followed by KEY=VALUE entries
// from extra, in deterministic (sorted) order.
func mergeEnv(base []string, extra map[string]string) []string {
	if len(extra) == 0 {
		out := make([]string, len(base))
		copy(out, base)
		return out
	}
	keys := make([]string, 0, len(extra))
	for k := range extra {
		keys = append(keys, k)
	}
	sort.Strings(keys)
	out := make([]string, 0, len(base)+len(extra))
	out = append(out, base...)
	for _, k := range keys {
		out = append(out, k+"="+extra[k])
	}
	return out
}
