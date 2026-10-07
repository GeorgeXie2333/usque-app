<p align="center">
  <img src="assets/branding/usque-readme-banner.png" alt="Usque — Cloudflare® WARP® サービスと互換性のある非公式クライアント" width="100%">
</p>

<p align="center">
  <a href="README.md">English</a>
  ·
  <a href="README.zh-CN.md">简体中文</a>
  ·
  日本語
  ·
  <a href="README.ko.md">한국어</a>
  ·
  <a href="README.ru.md">Русский</a>
  ·
  <a href="README.fa.md">فارسی</a>
</p>

<p align="center">
  <a href="https://github.com/GeorgeXie2333/usque-app/actions/workflows/pr-check.yml"><img alt="PR Check" src="https://github.com/GeorgeXie2333/usque-app/actions/workflows/pr-check.yml/badge.svg"></a>
  <a href="https://github.com/GeorgeXie2333/usque-app/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/GeorgeXie2333/usque-app/actions/workflows/ci.yml/badge.svg?branch=main"></a>
  <a href="https://github.com/GeorgeXie2333/usque-app/actions/workflows/build.yml"><img alt="Build" src="https://github.com/GeorgeXie2333/usque-app/actions/workflows/build.yml/badge.svg"></a>
  <a href="LICENSE.md"><img alt="MIT License" src="https://img.shields.io/badge/license-MIT-C2500C.svg"></a>
</p>

# Usque

Usque は Windows と Android / Android TV 向けのCloudflare® WARP® サービスと互換性のある非公式クライアントです。システム VPN、SOCKS5、HTTP プロキシをネイティブの Flutter インターフェースにまとめ、Rust 製の MASQUE エンジンで通信を処理します。

> [!IMPORTANT]
> 正式なパッケージは [GitHub Releases](https://github.com/GeorgeXie2333/usque-app/releases) からのみダウンロードしてください。Pull Request の成果物、ローカルビルド、タグのないバイナリは正式リリースではありません。開発ブランチの文書には未公開の変更が含まれる場合があります。使用するパッケージのタグに対応したリリースノートと文書を確認してください。

Usque は独立したプロジェクトです。Cloudflare, Inc. との提携関係はなく、同社からの支援や推奨も受けていません。 Cloudflare および WARP は、米国およびその他の法域における Cloudflare, Inc. の商標または登録商標です。 個人向け WARP サービスの利用には、引き続き Cloudflare の利用規約とプライバシーポリシーが適用されます。

## スクリーンショット

<table>
  <tr>
    <td align="center" valign="top">
      <p><strong>Windows</strong></p>
      <img src="assets/screenshots/usque-windows-home.png" alt="Windows 版 Usque のホーム画面" width="720">
    </td>
    <td align="center" valign="top">
      <p><strong>Android</strong></p>
      <img src="assets/screenshots/usque-android-home.png" alt="Android 版 Usque のホーム画面" width="280">
    </td>
  </tr>
</table>

現在のソースから描画した英語インターフェースのプレビューです。未接続の状態を表示しています。

## ダウンロードとインストール

この文書は **v0.3.0** の機能を説明し、ソースに記載されたアプリのバージョンは **0.3.0** です。バージョン表記だけでは公開済みとは限りません。承認された公式パッケージは [GitHub Releases](https://github.com/GeorgeXie2333/usque-app/releases) から取得してください。[リリース準備レビュー](docs/RELEASE_V0.3.0_READINESS.md) には当時のソースと検証範囲が記録されています。インストール用パッケージは 6 種類あり、別途 2 つの Windows MSI ファイルをアプリ内更新専用として提供します。

| プラットフォーム | 最低 OS 要件 | パッケージ |
| --- | --- | --- |
| Windows | Windows 10 22H2、ビルド 19045 | x64-v2 または ARM64 の EXE インストーラー |
| Android / Android TV | Android 8.0、API 26 | arm64-v8a、x86_64、armeabi-v7a の APK |
| Android / Android TV | Android 8.0、API 26 | 上記 3 種類の ABI を含むユニバーサル APK |

端末のアーキテクチャに合うパッケージを選んでください。Windows x64 には **x86-64-v2** 対応 CPU が必要で、ARM64 Windows にはネイティブ ARM64 パッケージを使用します。Android の ABI が不明な場合は、サイズの大きいユニバーサル APK を使用できます。インストール前に、パッケージの SHA-256 を `SHA256SUMS` と GitHub に表示されるファイルのダイジェストに照合し、リリースノートに記載された署名者のフィンガープリントも確認してください。いずれかが一致しない場合は、インストールを中止してください。

1.0 より前のパッケージは、プロジェクトで管理する固定の自己署名証明書を使用します。Windows では発行元が不明という警告が表示されることがあります。Android パッケージは Google Play 外からインストールします。警告を回避するためにウイルス対策やファイアウォールを無効にしたり、非公式パッケージに付属する証明書をインポートしたりしないでください。

アップグレード、アンインストール、復旧、Android の開発者確認については[インストールと削除](docs/INSTALLATION.md)、正式な署名に使う証明書については[コード署名](docs/CODE_SIGNING.md)を参照してください。更新のダウンロードには確認が必要で、インストールには各プラットフォームのインストーラーを使用します。無人での自動インストールは行いません。

アップグレードするとローカル設定は v0.2.9 が読めない形式へ移行します。事前に[設定の互換性](docs/INSTALLATION.md#configuration-compatibility-when-upgrading)と必要なバックアップを確認してください。古いパッケージの再インストールでは移行を元に戻せません。

## 初めての接続

1. [検証済みの正式パッケージ](docs/INSTALLATION.md#verify-before-installing)をインストールし、Usque を開きます。
2. 初回起動の権限と利用規約の手順を完了します。Android ではセットアップの完了に VPN の許可が必要です。許可すると別の VPN が切断される場合がありますが、それだけでは Usque の接続は開始されません。通知の許可は任意です。個人向け WARP® アカウントを登録し、必要に応じて WARP License Key を入力します。セットアップが中断された場合は、再登録の前に保存済みの結果を確認してください。新しい WARP Secret のインポートには対応していません。
3. Windows では **プロキシ → 仮想ネットワークアダプター とローカルプロキシ**、Android では **プロキシ → VPN とローカルプロキシ** を開き、使用する接続方式を選んでホームから接続します。スイッチは即座に反映されます。待ち受けアドレスや DNS のフォームを編集した場合は、**変更を適用** が必要です。プロキシのみの動作では、取得済みの VPN 権限を使って VPN を開始することはありません。

| 接続方式 | 用途 |
| --- | --- |
| VPN/TUN | バイパス規則と Android のアプリ別設定に従い、システムの通信をトンネル経由にします。 |
| SOCKS5 | 対応するアプリにローカル TCP/UDP プロキシを提供します。DNS は既定でリモート解決を使用します。 |
| HTTP プロキシ | 対応するアプリにローカル HTTP プロキシを提供します。CONNECT による HTTPS 接続も利用できます。 |
| Windows システムプロキシ | Windows のプロキシ設定を Usque の HTTP プロキシに向けます。HTTP の接続方式を有効にする必要があります。 |

VPN、SOCKS5、HTTP は既定で有効、Windows システムプロキシは無効です。
これらは 1 つの WARP 接続を共有し、同時に使用できます。すべて無効にしてもトランスポート接続は維持されますが、これらのアプリ向け接続方式は提供されなくなります。
認証情報はアカウントごとに保存し、一度に接続できるアカウントは 1 つです。
ネットワーク設定はすべてのアカウントで共有します。

## 主な機能

- 任意で利用できる[チェーンプロキシ](docs/CHAIN_PROXY.md)。**プロキシ → チェーンプロキシ** から、
  **OpenVPN**、**WireGuard**、[**WARP via WireGuard**](docs/WARP_WIREGUARD.md)、
  **VPN Gate**、**HTTP**、**SOCKS5** の順に出口を選べます。
  VPN 設定をインポートするかプロキシサーバーを追加し、1 つを選択して適用します。
  VPN、SOCKS5、HTTP は同じ最終出口を使用します。明示的な直結規則は引き続き適用されます。既定では無効です。
- 手動で有効にする[実験的な L4 モード](docs/L4_PROXY.md)で、HTTP/3 経由の TCP プロキシを利用できます。
  OpenVPN TCP のチェーン出口がない場合、通常の UDP は転送しません。UDP が必要なアプリは動作しないことがあります。
  自動モードでは L4 を選びません。
- HTTP/3 に自動接続し、失敗時は HTTP/2 にフォールバックします。IPv4 と IPv6 の接続試行で到達可能なエンドポイントを探します。
  対応するネットワーク変更では H3 接続を移行できます。[経路の動作](docs/h3-path-infrastructure.md)を参照してください。
- フルトンネル VPN、トンネル内 DNS、Kill Switch、LAN アクセス、[カスタム CIDR・IP・ドメインのバイパス規則](docs/encrypted-direct-dns.md#custom-bypass-targets--自定义绕过目标)。ドメイン規則はサブドメインも含み、国別規則のダウンロードなしで利用できます。
- [WARP 出口 DNS](docs/WARP_DNS.md) をカスタマイズ：**設定 → 高度なネットワーク設定 → IP と DNS** を開き、通常の DNS、DoH、DoT を選んで **変更を適用** します。DNS を変更すると接続中のセッションを再接続します。最終チェーン出口は独自の DNS 方針を維持します。
- 任意の国別直結ルーティング。選択した国の GeoIP データと全体の GeoSite カタログを別々にダウンロードします。
  ドメイン名が見える場合はドメイン規則、それ以外は IP 規則で判定し、分類できない宛先はトンネル経由のままにします。
- ローカルの[ネットワーク診断](docs/network-doctor.md)とネットワーク品質ページ。遅延、パケットロスと測定の可否、キュー、直近 60 秒の推移を表示します。
  標準検査はローカル状態の読み取りのみです。詳細検査は確認後にテストリクエストを送信します。
- Windows のタスクトレイ（状態バッジ、仮想ネットワークアダプターとシステムプロキシのスイッチ、バックグラウンドで 5 秒間続く再接続・接続エラー・通知済み中断からの復旧を通知）、単一インスタンス、起動時の自動実行、ウィンドウを閉じてトレイへ最小化する機能、ウィンドウ位置の記憶、キーボードショートカット。
  **Ctrl+1～4** でページ切替、**Ctrl+S** で変更適用、**F5** で VPN Gate または診断を更新します。[トレイとキーボード操作](docs/INSTALLATION.md#tray-and-keyboard-controls)を参照してください。
  Android のクイック設定タイル、ランチャーのショートカット、再起動後の復帰、TV のリモコン操作。
  21 言語とライト・ダークテーマに対応しています。
- 確認後、個人向け WARP Secret を指定したファイルにエクスポートできます。
  Usque はそのファイルのインポートに対応していないため、再インストール後のアカウント復元には使えません。

Android の **アプリごとのプロキシ** は、アカウントに関係なくアプリ全体に適用されます。無効時はすべてのアプリが VPN を使用します。
有効時は選択したアプリのみが使用し、新しくインストールしたアプリは手動で選択する必要があります。
Android の **VPN 以外の接続をブロック** も有効にしている場合、未選択のアプリはトンネルを迂回するのではなく通信をブロックされます。

## プライバシーと制限

- WARP サーバーの公開鍵は登録済みの鍵と一致する必要があります。検証を省略する設定はありません。
  認証情報は Windows 資格情報マネージャーまたは Android Keystore に保存します。
  Windows では別の Agent が特権を必要とするネットワーク操作を行い、Android では専用プロセスで VPN を実行します。
- プロキシの待ち受けは既定でループバックのみです。SOCKS5 と HTTP は任意のユーザー名・パスワード認証に対応し、認証情報を設定しなければ認証を要求しません。
  プロキシのみのモードでは、システム全体を対象とする VPN Kill Switch は提供されません。
- 診断はローカルで生成し、機密情報を除去します。利用状況の分析や自動アップロードは行わず、品質履歴はメモリ内にのみ保持します。
  ログの既定レベルは INFO で、保存上限は 7 日または 20 MiB です。認証情報や未処理の診断バンドルを公開 Issue に投稿しないでください。
  脆弱性は [SECURITY.md](SECURITY.md) の手順で非公開報告してください。
- Android のアプリ内 Kill Switch は、VPN プロセスの終了後には通信を保護できません。VPN Gate で接続を継続できない障害が起きた場合も、接続は終了します。
  VPN 終了後もアプリの通信をブロックするには、システムの **常時接続 VPN** と **VPN 以外の接続をブロック** の両方を有効にしてください。
  [Android のセットアップ](docs/INSTALLATION.md#android-and-android-tv)を参照してください。
- 複数の経路を束ねて帯域幅を増やすことはできません。HTTP/2 のパケットロスや PMTU など、利用できない品質指標があります。
  ローカル診断の成功は、通信漏れがないことや性能が向上したことの証明にはなりません。

### DNS のプライバシー

国別規則とカスタムドメインによる直結では、既定で **System** DNS を使用します。規則に一致したドメインの問い合わせは、VPN の外側にある現在のネットワークの DNS サーバーへ送られます。
代わりに **DoH** または **DoT** を選び、暗号化 DNS リゾルバーの名前と IP アドレスを指定できます。
そのリゾルバーが問い合わせを受け取り、接続失敗時に平文 DNS へ切り替えることはありません。
[設定手順と例](docs/encrypted-direct-dns.md)を参照してください。

その他のリモート VPN 問い合わせには WARP トンネルまたは選択した最終チェーン出口を使用します。
HTTP/SOCKS5 のチェーン DNS は、既定でそのプロキシ経由の TLS 検証付き Cloudflare® DoH を使用します。
カスタム DNS や既定値以外の継承 DNS は TCP DNS を維持します。この 2 種類の出口では、アプリが選んだリゾルバーへの UDP/53 問い合わせを、同じリゾルバーへの TCP 問い合わせに変換します。物理ネットワークの DNS にはフォールバックしません。
[チェーン DNS の選択肢](docs/CHAIN_PROXY.md#http-and-socks5-exits--http-与-socks5-出口)を参照してください。
明示的なローカル DNS とプロキシ DNS の設定も引き続き適用されます。
アプリが独自の暗号化 DNS を使用すると Usque にはドメイン名が見えないため、直結ルーティングには IP 規則を使用します。
切断中の規則ダウンロードも、Android の Lockdown と Windows に残る Kill Switch の制約に従います。

### 実験的な機能と未対応の範囲

[Zero Trust 登録](docs/ZERO_TRUST_EXPERIMENTAL.md)は実験的な機能です。組織の ID を使って MASQUE インターネットトンネルを構成しますが、Cloudflare One™ Client の全機能との互換性は提供しません。
macOS のソースは保持していますが、ビルドやリリースは行いません。
iOS、アプリストアでの配布、公開 CLI は今回のリリース範囲に含まれません。

## 既定のネットワーク設定

| 設定 | 既定値 |
| --- | --- |
| 個人向けエンドポイントの選択 | 自動選択。既存の設定はカスタムを維持 |
| 保存済みカスタム IPv4 エンドポイント | `162.159.198.2` |
| 保存済みカスタム IPv6 エンドポイント | `2606:4700:103::2` |
| ポート / SNI | `443` / `speed.cloudflare.com` |
| トランスポート | 自動: HTTP/3、次に HTTP/2 |
| HTTP/3 輻輳制御 | `cubic`。BBRv2、実験的な BBRv3、`reno` も選択可能 |
| QUIC UDP 受信バッファ | Windows/Android の H3 と L4 で [2 MiB を目標](docs/UDP_RECEIVE_BUFFER.md)として要求。実際の容量は OS に依存 |
| TUN MTU | `1280` |
| フォールバック DNS | `1.1.1.1`、`2606:4700:4700::1111` |
| SOCKS5 | `127.0.0.1:1080`、`[::1]:1080` |
| HTTP プロキシ | `127.0.0.1:8080`、`[::1]:8080` |

プロキシのアドレス、ポート、DNS の編集は、変更を適用するまで下書きです。高度な設定のリセットは既定値を下書きに読み込むだけで、即座には適用しません。Zero Trust は初期状態で登録時のアドレスを使用します。高度なネットワーク設定で **Zero Trust エンドポイントを編集** を選び、赤い全画面の警告を読み、リスクと利用権限を確認してから IPv4/IPv6 を編集し、**変更を適用** します。リセットは登録時のアドレスを下書きに戻し、再ログインはカスタムアドレスを削除して登録時のアドレスを復元します。 選択中のアカウントにカスタム Zero Trust アドレスがある場合、または現在の ZT 接続がそれを使用している間は、ホームに閉じられないリスク警告が表示されます。

高度なネットワーク設定の自動選択は、アカウントで利用可能なエンドポイントへの接続を並行して試みます。カスタムでは手動のアドレスを維持します。ポートと SNI はどちらでも編集できます。[自動エンドポイント選択](docs/NETWORK_SETTINGS.md#automatic-endpoints--自动选择端点)を参照してください。

輻輳制御の変更は保存され、次の手動接続または再試行で有効になります。現在のセッションとその自動再接続には適用されません。
HTTP/2 はシステムの TCP を使用します。[HTTP/3 輻輳制御](docs/congestion-control.md)を参照してください。

## 文書と開発

セットアップや実用的な手順は [Wiki](https://github.com/GeorgeXie2333/usque-app/wiki/Home) から始めてください。[文書一覧](docs/README.md)にはすべての参考資料があります。技術文書の多くは英語です。

| 知りたいこと | 参照先 |
| --- | --- |
| インストール、更新、削除、復旧 | [インストール](docs/INSTALLATION.md) |
| WARP 経由で Proton VPN に接続 | [WireGuard over MASQUE チュートリアル](https://github.com/GeorgeXie2333/usque-app/wiki/Proton-VPN-over-MASQUE) |
| ローカル品質検査の意味 | [Network Doctor](docs/network-doctor.md) |
| 変更を安全にビルド・テスト | [貢献ガイド](CONTRIBUTING.md) |
| 実装と検証の状況 | [実装状況](docs/IMPLEMENTATION.md) |
| 正式リリースの保守 | [リリース手順](docs/RELEASE.md) |

貢献ガイドの固定ツールチェーンと、変更範囲に応じた検査を使用してください。コンパイルのみのビルドと決定論的なテストは開発用 PC で実行できます。
開発パッケージのインストールや VPN ライフサイクルの検証には、指定された隔離環境が必要です。ビルド成功は、インストール、通信漏れ、性能の検証を意味しません。

## 上流プロジェクトとライセンス

プロトコルの動作は [Diniboy1123/usque](https://github.com/Diniboy1123/usque) を参考にしています。このリポジトリは相互運用性テスト用に、そのクライアントのスナップショットを `oracle/go` に保存しています。Flutter UI と Rust エンジンは新たに実装したものです。上流の著作権表示はライセンスに残しています。

本プロジェクトのソースは [MIT](LICENSE.md) ライセンスです。サードパーティのコンポーネントはそれぞれのライセンスを維持します。
任意の[チェーンプロキシ](docs/CHAIN_PROXY.md)は、MPL-2.0 の OpenVPN 3 Core と Apache-2.0 の Mbed TLS を組み込みます。
対応するソース、レビュー済みのパッチ、ライセンス本文は `third_party` に含まれ、アプリの VPN Gate ページからライセンス表示を確認できます。
WireGuard は BoringTun 0.7.1（BSD-3-Clause）、ローカル SVG アイコンは flutter_svg 2.3.0（MIT）を使用します。これらのライセンス表示もアプリのライセンス一覧に含まれています。

---

Cloudflare、WARP および Cloudflare One は、米国およびその他の法域における Cloudflare, Inc. の商標または登録商標です。
