#!/usr/bin/env bash
# The release and disposable CI fixture both exercise this native signing path.
set -euo pipefail
umask 077
arch=${1:?expected architecture}
tag=${2:?expected version tag}
mode=${3:?expected release or fixture}
test "${RUNNER_ENVIRONMENT:-}" = github-hosted
test "$(uname -s)" = Darwin
test "$(uname -m)" = "$arch"
work=$(mktemp -d "$RUNNER_TEMP/usque-sign.XXXXXXXX")
keychain="$work/signing.keychain-db"
trusted=0
cleanup() {
  result=$?
  if [[ "$trusted" == 1 ]]; then
    sudo -n security remove-trusted-cert -d "$work/leaf.der" || result=1
  fi
  if [[ -e "$keychain" ]]; then security delete-keychain "$keychain" || result=1; fi
  rm -rf -- "$work" || result=1
  exit "$result"
}
trap cleanup EXIT
case "$mode" in
  release)
    test "$GITHUB_REF" = "refs/tags/$tag"
    test "$GITHUB_EVENT_NAME" = push
    test "$(git rev-parse HEAD)" = "$GITHUB_SHA"
    expected=$(cat packaging/macos/release-certificate.sha256)
    printf '%s' "${MACOS_SIGNING_P12_BASE64:?missing protected PKCS#12}" | /usr/bin/base64 -D > "$work/identity.p12"
    unset MACOS_SIGNING_P12_BASE64
    : "${MACOS_SIGNING_P12_PASSWORD:?missing protected password}"
    ;;
  fixture)
    # This job has no production credentials or release environment.
    MACOS_SIGNING_P12_PASSWORD=$(openssl rand -hex 32)
    export MACOS_SIGNING_P12_PASSWORD
    printf '%s\n' '[req]' 'distinguished_name=dn' '[dn]' '[signer]' \
      'basicConstraints=critical,CA:FALSE' 'keyUsage=critical,digitalSignature' \
      'extendedKeyUsage=critical,codeSigning' > "$work/openssl.cnf"
    /usr/bin/openssl req -x509 -newkey rsa:2048 -nodes -days 1 -sha256 \
      -subj '/CN=Usque disposable CI signer' -config "$work/openssl.cnf" -extensions signer \
      -keyout "$work/fixture.key" -out "$work/fixture.pem"
    /usr/bin/openssl pkcs12 -export -inkey "$work/fixture.key" -in "$work/fixture.pem" \
      -keypbe PBE-SHA1-3DES -certpbe PBE-SHA1-3DES -macalg sha1 \
      -passout env:MACOS_SIGNING_P12_PASSWORD -out "$work/identity.p12"
    expected=$(/usr/bin/openssl x509 -in "$work/fixture.pem" -outform DER | shasum -a 256 | cut -d' ' -f1)
    ;;
  *) exit 2 ;;
esac
/usr/bin/openssl pkcs12 -in "$work/identity.p12" -passin env:MACOS_SIGNING_P12_PASSWORD \
  -clcerts -nokeys -out "$work/certificate.pem"
/usr/bin/openssl x509 -in "$work/certificate.pem" -outform DER -out "$work/leaf.der"
actual=$(shasum -a 256 "$work/leaf.der" | cut -d' ' -f1)
test "$actual" = "$expected"
# codesign selects identities by SHA-1. The trust pin above and the checks on
# emitted signatures use SHA-256 of the DER leaf.
identity=$(shasum -a 1 "$work/leaf.der" | cut -d' ' -f1)
password=$(openssl rand -hex 32)
security create-keychain -p "$password" "$keychain"
security set-keychain-settings -lut 3600 "$keychain"
security unlock-keychain -p "$password" "$keychain"
security import "$work/identity.p12" -k "$keychain" -P "$MACOS_SIGNING_P12_PASSWORD" -T /usr/bin/codesign
unset MACOS_SIGNING_P12_PASSWORD
security set-key-partition-list -S apple-tool:,apple: -s -k "$password" "$keychain" >/dev/null
# Trust this exact signer exclusively for code signing on this disposable runner.
sudo -n security add-trusted-cert -d -r trustRoot -p codeSign -k "$keychain" "$work/leaf.der"
trusted=1
python3 tool/macos_release.py --app "$RUNNER_TEMP/mac-app/Usque.app" --arch "$arch" \
  --tag "$tag" --output "$RUNNER_TEMP/mac-dmg" --keychain "$keychain" \
  --identity "$identity" --fingerprint "$expected"
