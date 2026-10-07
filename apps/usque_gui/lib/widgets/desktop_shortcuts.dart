import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';

/// Keyboard shortcuts are a desktop convention; Android and TV keep D-pad
/// and system navigation only.
bool get desktopShortcutsSupported =>
    defaultTargetPlatform == TargetPlatform.windows ||
    defaultTargetPlatform == TargetPlatform.linux;

/// True when [context] belongs to the top route of every enclosing navigator,
/// so a dialog, popup menu, or pushed subpage hides it from shortcuts.
bool routeChainIsCurrent(BuildContext context) {
  BuildContext? current = context;
  while (current != null) {
    final route = ModalRoute.of(current);
    if (route == null) return true;
    if (!route.isCurrent) return false;
    current = route.navigator?.context;
  }
  return true;
}

/// True while a text field owns the keyboard, where Escape and Alt+Left
/// already mean something.
bool get textInputHasFocus =>
    FocusManager.instance.primaryFocus?.context
        ?.findAncestorWidgetOfExactType<EditableText>() !=
    null;

/// Runs [onInvoke] for [activator] while this subtree is the visible, top
/// route.
///
/// Uses a [HardwareKeyboard] handler instead of [Shortcuts] so the binding
/// works whichever control has focus, including none. Hidden shell sections
/// mute their tickers, which also mutes these bindings.
class PageShortcut extends StatefulWidget {
  const PageShortcut({
    required this.activator,
    required this.onInvoke,
    required this.child,
    super.key,
  });

  final SingleActivator activator;

  /// Null disables the binding, mirroring a disabled button.
  final VoidCallback? onInvoke;
  final Widget child;

  @override
  State<PageShortcut> createState() => _PageShortcutState();
}

class _PageShortcutState extends State<PageShortcut> {
  bool _visible = true;
  bool _registered = false;

  @override
  void initState() {
    super.initState();
    if (desktopShortcutsSupported) {
      HardwareKeyboard.instance.addHandler(_handle);
      _registered = true;
    }
  }

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    _visible = TickerMode.valuesOf(context).enabled;
  }

  @override
  void dispose() {
    if (_registered) HardwareKeyboard.instance.removeHandler(_handle);
    super.dispose();
  }

  bool _handle(KeyEvent event) {
    final onInvoke = widget.onInvoke;
    if (onInvoke == null ||
        !mounted ||
        !_visible ||
        event is! KeyDownEvent ||
        !widget.activator.accepts(event, HardwareKeyboard.instance) ||
        !routeChainIsCurrent(context)) {
      return false;
    }
    onInvoke();
    return true;
  }

  @override
  Widget build(BuildContext context) => widget.child;
}
