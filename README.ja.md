<p align="center">
  <img src="resources/logo_128.png" width="120" alt="Oryxis logo">
</p>

<h1 align="center">Oryxis</h1>

<p align="center">
  Rust だけで作られたモダンな SSH クライアント。高速、暗号化、ネイティブ。
</p>

<p align="center">
  <a href="README.md">English</a> | <a href="README.zh-CN.md">简体中文</a> | <a href="README.zh-TW.md">繁體中文</a> | 日本語 | <a href="README.ko.md">한국어</a> | <a href="README.fa.md">فارسی</a> | <a href="README.pt-BR.md">Português (BR)</a>
</p>

<p align="center">
  <a href="https://github.com/wilsonglasser/oryxis/releases/latest"><img src="https://img.shields.io/github/v/release/wilsonglasser/oryxis?color=green" alt="Release"></a>
  <img src="https://img.shields.io/badge/platforms-linux%20%7C%20macos%20%7C%20windows-blue" alt="Platforms">
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-AGPL--3.0-blue" alt="License"></a>
  <a href="https://oryxis.app"><img src="https://img.shields.io/badge/website-oryxis.app-3CBBB1" alt="Website"></a>
</p>

<p align="center">
  <img src="resources/screen_1.gif" width="720" alt="Oryxis の動作例：ホストへの接続、スニペットの実行、SFTP ブラウズ">
</p>

> このドキュメントは v0.23.0 以降の英語版 README の翻訳です（2026-10-29 同期）。
> 詳細ドキュメント（[機能ツアー](docs/FEATURES.md)、[アーキテクチャ](docs/ARCHITECTURE.md)）は英語です。

## Oryxis とは？

Oryxis は [Termius](https://termius.com/) のオープンソース代替です。
モダンな UI、認証情報を保存するローカル暗号化ボールトを備えたデスク
トップ SSH クライアントで、クラウドアカウントは一切登場しません。
Electron なし、webview なし、ベンダーサーバーなし。単一のネイティブ
バイナリだけです。

|  | Oryxis | Termius | PuTTY | Tabby |
|--|--------|---------|-------|-------|
| UI スタック | ネイティブ Rust（iced + wgpu） | Electron | ネイティブ | Electron |
| ライセンス | AGPL-3.0、オープンソース | プロプライエタリ | MIT | MIT |
| 認証情報の保存 | ローカル暗号化ボールト | ベンダーのクラウドアカウント | なし | ローカル設定ファイル |
| デバイス同期 | P2P・E2E 暗号化、セルフホスト中継も可 | ベンダークラウド（サブスク） | なし | Tabby Web 経由 |
| SFTP | デュアルペイン GUI **と**対話型コンソール | 有料プラン | CLI のみ | 簡易パネル |
| 価格 | 無料 | 無料枠 + サブスク | 無料 | 無料 |

## インストール

**Windows**

[![Microsoft Store から入手](https://get.microsoft.com/images/ja%20dark.svg)](https://apps.microsoft.com/detail/9NTKPPSHBTG2)

またはターミナルから:

```powershell
winget install WilsonGlasser.Oryxis
```

**Arch Linux (AUR)**

```bash
yay -S oryxis-bin
```

**直接ダウンロード**：[最新リリース](https://github.com/wilsonglasser/oryxis/releases/latest)から、
Linux（`.tar.gz` / `.deb` / `.AppImage`、x86_64 と ARM64）、
macOS（Apple Silicon `.dmg`）、Windows（システム / ユーザー単位の
インストーラーとポータブル `.zip`、x86_64 と ARM64）を提供しています。
Windows バイナリは Authenticode 署名済みです。ポータブル `.zip` は、
コンピューターに Oryxis のプロファイルが既にある場合を除き、すべて
（設定、ボールト、プラグイン、ログ）を `oryxis.exe` の隣に作成する
`data` フォルダーに保存します。Linux と macOS では、同じ目的でバイナリ、
`.AppImage` ファイル、または `Oryxis.app` の隣に `oryxis-data` フォルダーを
作成してください。

### フォントとエンコーディング

UI 言語を日本語に切り替えると、Noto Sans JP フォントが自動でダウン
ロードされます（オンデマンド方式なのでインストーラーのサイズには
影響しません）。ネットワーク機器などのレガシーな装置に接続する場合
は、ホストごとに Shift_JIS や EUC-JP といったエンコーディングを選択
できます。

## ハイライト

- **ネイティブで高速。** 純 Rust、GPU アクセラレーションの
  [iced](https://iced.rs) UI、単一バイナリ。Electron も webview も
  ありません。
- **ローカル暗号化ボールト。** Argon2id + ChaCha20-Poly1305 による
  フィールド単位の暗号化、任意のマスターパスワード、生体認証
  アンロック（Windows Hello / Touch ID / Linux キーリング）、
  アイドル時自動ロック、2FA ホスト向け TOTP 自動入力、`sudo` の
  パスワード要求で保管庫のパスワードを提示（自動送信はしません）。
- **複数のボールト。** 仕事用と個人用を別々の暗号化ファイルに分け、
  それぞれが自分のマスターパスワード、設定、同期、MCP への公開を
  持ったまま同時に開けます：各ウィンドウは一つのボールトで作業し、
  ホームタブ、ボールトごとのショートカット、またはパレットからその
  場で切り替えられ、もう一方のボールトのタブは動き続けます。二つの
  ボールトが鍵を共有することはありません。
- **フルの SSH パイプライン。** 自動認証、多段ジャンプホスト、
  SOCKS / HTTP / コマンドプロキシ、エージェント転送、独立した
  `-L`/`-R`/`-D` ポートフォワード、メニュー型踏み台（JumpServer
  など）向けの expect/send ログインスクリプト。
- **セキュリティキーをネイティブで。** YubiKey などの FIDO2 トークン
  が、Windows、Linux、macOS（Windows Hello を含む）でアプリ自身から
  `sk-ssh-ed25519` / `sk-ecdsa-sha2-nistp256` のログインに署名します。
  タッチと PIN はあなたの画面上で求められ、ハードウェア専用の
  「セキュリティキー」認証方式を備え、間に外部エージェントは挟まり
  ません。
- **ホストをそのまま持ち込む。** 手元にあるものを一度のインポートで
  読み込みます：`~/.ssh/config`、PuTTY、KiTTY、WinSCP、mRemoteNG、
  MobaXterm、SecureCRT、Xshell、FinalShell、Termius、任意の CSV。
  ファイル（またはセッションフォルダー）を選ぶだけで、形式は自動で
  判別されます。編集したホスト CSV のエクスポートは、更新、削除、
  フォルダーとして取り込まれます。
- **オフラインはあなたの指示で。** 初回起動時と設定で提供される
  スイッチ一つで、Oryxis は自らリクエストを送らなくなります：更新の
  確認も、フォントやプラグインのダウンロードもしません。自分で設定
  したものはそのまま動きます。一度もネットワークにつながったことの
  ないマシン向けには、プラグインとフォントパックを同梱しスイッチを
  あらかじめオンにしたオフラインバンドルが、毎リリースに並んで配布
  されます。
- **SSH だけではなく。** Telnet とシリアルコンソール、コンソール
  サーバー向けの生 TCP 接続、ZMODEM 転送、ローカルシェル、SSH
  トンネル経由のワンクリック RDP/VNC。
- **ネットワークが変わっても続くセッション。** ホストで mosh を
  有効にすると、シェルはスリープも Wi-Fi の切り替えもアドレスの
  変更も乗り越えます。しかも何事もないふりをせず、リンクがどれだ
  け途絶えているかを画面が伝えます。純正 `mosh-server` のプロトコル
  を話すネイティブ Rust クライアントなので、手元に追加でインス
  トールするものはありません。
- **本物のターミナル。** alacritty ベースのエミュレーター、分割
  ペイン、セッショングループ、ホストごとのテーマ、任意の半透明背景
  または背景画像、同梱 Nerd Fonts に加えてダウンロード式フォント
  パック（JetBrains Mono、Fira Code、MesloLGS など）、長時間コマンド
  を知らせるスマートタブ、入力中に候補として出るホストごとのコマンド
  履歴、好みの形のカーソル、そして CJK の TUI の表示を揃えるホスト
  ごとの東アジアの曖昧幅（ambiguous width）設定。
- **順に実行するスニペット。** 一つのスニペットでコマンドを入力し、
  コマンドの終了や出力中の行を待ち、出力をコピーまたは保存し、ほか
  のスニペットを呼び出せます。すべて起動したターミナル上で動き、変
  数は一度だけ尋ねられます。よく使うものは、ターミナルの下の
  スニペットバーにボタンとして置けます。
- **ウィンドウは好きなだけ。** 一つのプロセスに任意の数のウィンドウ。
  それぞれが自分のタブ、ホスト画面、検索を持ちます。タブを端の外へ
  ドラッグすれば切り離し、別のウィンドウのタブバーに落とせばドッキ
  ング、ホストを直接新しいウィンドウに接続することもできます。
- **ファイルはどこでも。** ドラッグ&ドロップ、シンボリックリンク、その場編集、サーバー間
  コピーに対応したデュアルペイン SFTP。各 SSH タブにはシェルの作業
  ディレクトリに追従する Files サイドバーもあります。タイピング派に
  は、`sftp(1)` のコマンド（`get`、`put`、`mget`、`lcd`、グロブ、Tab
  補完、インライン進捗）を話す対話型 SFTP コンソールがあり、いま開い
  ているセッションのペインとして（下に、横に、または最大化で、お好
  みで）開き、ターミナル・コンソール・ファイルをスイッチ一つで切り
  替えられます。
- **セッション録画。** 保存時に暗号化。asciinema `.cast`（テーマ
  埋め込み）またはプレーンテキストにエクスポート。設計上、出力のみ
  を記録します。
- **システム管理者の道具箱。** 任意のネットワークツールパネル（既定
  はオフ、独立したタブとして開きます）：DNS レコード、ping、
  traceroute、TCP ポートテスト、HTTP リダイレクトの連鎖と証明書の
  検査、WHOIS、公開スパムブロックリスト。
- **クラウドアカウント。** AWS、Google Cloud、Azure、Alibaba Cloud、
  Tencent Cloud、Kubernetes のリソース検出と接続（EC2、SSM、ECS Exec、
  GKE、AKS、ACK、TKE、`kubectl`）。署名付きプラグインとしてオンデマ
  ンド配布。
- **仕事場に AI を。** タブごとのアシスタント（API キー持ち込み：
  Anthropic、OpenAI、Gemini、互換 API）、多層の自動実行セーフティ、
  さらに [MCP サーバー](docs/FEATURES.md#mcp-server)で Claude Code
  などの AI クライアントにホストを公開し、そのうち 7 つには自らを
  インストールでき、許可すればホストの追加や編集も行い、root での
  コマンド実行は画面上であなたが承認してからに限られます。
- **P2P 同期、クラウドなし。** QUIC 上のエンドツーエンド暗号化
  （X25519 + XChaCha20-Poly1305）。LAN 内は mDNS、ネットワークを
  跨ぐ場合は[セルフホスト](SELF_HOSTING.md)のシグナリング / リレー。
  アカウントもベンダーサーバーも不要です。
- **キーボードファースト。** `user@host` クイック接続（Ctrl+K）、
  最近使った順またはタブバーの順で切り替える Ctrl+Tab、最後のトグ
  ルまで届くフルキーボードナビゲーション、全ホットキーの再割り当て。
  Windows Terminal、Tabby、iTerm2 のキーから始めることもできます。
- **プライバシーバイデザイン。** テレメトリは一切なし、プライバシー
  モードのマスキング、貼り付け内容を確認するペーストガード、完全な
  RTL 対応を含む [23 言語](docs/FEATURES.md#themes--internationalization)：
  English、Português、Español、Français、Deutsch、Italiano、简体中文、
  繁體中文、日本語、Русский、فارسی、العربية、עברית、한국어、Polski、
  Türkçe、Bahasa Indonesia、Tiếng Việt、Українська、ไทย、हिन्दी、
  Čeština、Ελληνικά。

全機能の一覧は英語版の[機能ツアー](docs/FEATURES.md)にあります。
tmux をお使いですか？**[tmux でのログとコマンド履歴](docs/TMUX.md)**（英語）が、そのまま動くものとご自身でインストールするものを説明しています。
ファイルブラウザーをシェルのディレクトリに正確に追従させたい場合は、**[シェルのディレクトリに追従する](docs/CWD.md)**（英語）にスニペットがあります。
ボールトのコピーをこのマシンの外、クラウドフォルダーなどへ持ち出したい場合は、**[バックアップと保管場所](docs/BACKUP.md)**（英語）が同期、エクスポート、そしてファイルを最後まで運ぶツールを説明します。

## クイックスタート

1. **初回起動：** マスターパスワードを設定するか、後回しにできます
   （生体認証アンロックとあわせて、あとから設定で有効化できます）。
2. **ホスト追加：** `+ HOST` をクリック、または `user@host` を入力
   （Ctrl+K）して保存せずに接続。別の SSH クライアントからの乗り換え
   なら、インポート一回で保存済みセッションをそのまま持ち込めます。
3. **接続：** ホストカードをクリック。分割ペイン、Files サイドバー、
   SFTP、スニペットはキー一つの距離にあります。
4. **オプション：** AI チャット（設定 > AI）、MCP サーバー
   （設定 > セキュリティ）、デバイス間 P2P 同期（設定 > 同期）。

質問があれば [FAQ](https://github.com/wilsonglasser/oryxis/discussions/66)
を見るか、[Discussion](https://github.com/wilsonglasser/oryxis/discussions)
を立ててください。

## セキュリティ

すべての機密データはフィールド単位で暗号化して保存され
（Argon2id + ChaCha20-Poly1305）、ホスト鍵は TOFU でピン留め、同期
ペイロードはエンドツーエンド暗号化、プラグインは実行前に Ed25519
署名を検証します。テレメトリは一切ありません。

セキュリティモデルの全体像と脆弱性報告ポリシーは
[SECURITY.md](SECURITY.md) にあります。脆弱性は非公開の経路で報告
してください。

## ロードマップ

Oryxis はおよそ週次で小さくリリースし、機能は準備ができ次第出荷
されます。最新の安定版は **v0.23.0**。履歴は
[CHANGELOG.md](CHANGELOG.md)、インタラクティブなロードマップは
[ロードマップ Discussion](https://github.com/wilsonglasser/oryxis/discussions/67)
にあります。0.15.0 以降に出荷されたもの：分割ペインと SFTP コンソー
ル、オフラインモードとそのバンドル、Alibaba Cloud と Tencent Cloud、
ネットワークツールパネル、ワンクリックのリレーデプロイ、ネイティブ
のセキュリティキー、一つのプロセスでの複数ウィンドウ、一連の
アクションを実行するスニペット、ほかのターミナルのショートカット
プリセット、破損したボールトの再構築、複数のボールト、タブバー
自身の順序で切り替える Ctrl+Tab、入力中に提案されるコマンド履歴、
スニペットバー、カーソルの形状と点滅、そして画面上で承認する MCP
プラグイン経由の sudo、`data` フォルダーを使うポータブルモード、
ボールトを保守するホスト CSV、SFTP のシンボリックリンク。進行中の方向性：AI 運用ツールキット。

## コントリビュート

コントリビューション歓迎です。日本語での issue や Discussion も
構いません（メンテナーは翻訳を使って読みます）。コード、コミット
メッセージ、コメントは英語でお願いします。開発環境、品質ゲート、
プロジェクトの規約は [CONTRIBUTING.md](CONTRIBUTING.md) を参照して
ください。

## ライセンス

Copyright (C) 2026 Wilson Glasser。
[AGPL-3.0-or-later](LICENSE) でライセンスされています。誰でも
Oryxis を使用、改変、再配布できますが、改変版をネットワーク越しに
提供する場合は、同じライセンスでソースコードを公開する必要があり
ます。詳細は [NOTICE](NOTICE) を参照してください。

---

<p align="center">
  ターミナルに住む人のために、Rust で作られています。
</p>
