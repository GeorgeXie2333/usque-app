# Chain import regression fixture

`proton_style_password_udp.ovpn` preserves the five ports, `remote-random`,
`CLIENT_CERT 0`, `mssfix 0` and password-authentication structure of the
user-supplied Proton configuration. It uses documentation-range endpoints and
the public in-memory test CA/key from `usque-openvpn`; account comments and
production identity material are absent. Never use it as a real VPN profile.
