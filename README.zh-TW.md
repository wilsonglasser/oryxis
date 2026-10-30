<p align="center">
  <img src="resources/logo_128.png" width="120" alt="Oryxis logo">
</p>

<h1 align="center">Oryxis</h1>

<p align="center">
  完全以 Rust 打造的現代 SSH 用戶端。快速、加密、原生。
</p>

<p align="center">
  <a href="README.md">English</a> | <a href="README.zh-CN.md">简体中文</a> | 繁體中文 | <a href="README.ja.md">日本語</a> | <a href="README.ko.md">한국어</a> | <a href="README.fa.md">فارسی</a> | <a href="README.pt-BR.md">Português (BR)</a>
</p>

<p align="center">
  <a href="https://github.com/wilsonglasser/oryxis/releases/latest"><img src="https://img.shields.io/github/v/release/wilsonglasser/oryxis?color=green" alt="Release"></a>
  <img src="https://img.shields.io/badge/platforms-linux%20%7C%20macos%20%7C%20windows-blue" alt="Platforms">
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-AGPL--3.0-blue" alt="License"></a>
  <a href="https://oryxis.app"><img src="https://img.shields.io/badge/website-oryxis.app-3CBBB1" alt="Website"></a>
</p>

<p align="center">
  <img src="resources/screen_1.gif" width="720" alt="Oryxis 實際操作：連線主機、執行程式碼片段、瀏覽 SFTP">
</p>

> 本文件譯自 v0.23.0 之後的英文 README（2026-10-29 同步），採用台灣慣用詞彙。
> 詳細文件（[功能總覽](docs/FEATURES.md)、[架構說明](docs/ARCHITECTURE.md)）為英文。

## Oryxis 是什麼？

Oryxis 是 [Termius](https://termius.com/) 的開源替代品：一款桌面 SSH
用戶端，擁有現代化介面與保存憑證的本機加密保險庫，整個流程沒有任何
雲端帳號。沒有 Electron、沒有 webview、沒有廠商伺服器，只有一個原生
二進位檔。

|  | Oryxis | Termius | PuTTY | Tabby |
|--|--------|---------|-------|-------|
| 介面技術 | 原生 Rust（iced + wgpu） | Electron | 原生 | Electron |
| 授權條款 | AGPL-3.0，開源 | 專有 | MIT | MIT |
| 憑證儲存 | 本機加密保險庫 | 廠商雲端帳號 | 無 | 本機設定檔 |
| 多裝置同步 | P2P 端對端加密，可自架中繼 | 廠商雲端（訂閱制） | 無 | 透過 Tabby Web |
| SFTP | 雙欄圖形介面**與**互動式主控台 | 付費方案 | 僅命令列 | 基本面板 |
| 價格 | 免費 | 免費版 + 訂閱 | 免費 | 免費 |

## 安裝

**Windows**

[![從 Microsoft Store 取得](https://get.microsoft.com/images/zh-tw%20dark.svg)](https://apps.microsoft.com/detail/9NTKPPSHBTG2)

或使用終端機：

```powershell
winget install WilsonGlasser.Oryxis
```

**Arch Linux (AUR)**

```bash
yay -S oryxis-bin
```

**直接下載**：前往[最新版本頁](https://github.com/wilsonglasser/oryxis/releases/latest)，
提供 Linux（`.tar.gz` / `.deb` / `.AppImage`，x86_64 與 ARM64）、
macOS（Apple Silicon `.dmg`）和 Windows（系統層級與使用者層級安裝程式、
可攜版 `.zip`，x86_64 與 ARM64）。Windows 二進位檔已完成 Authenticode
簽章。可攜版 `.zip` 會把一切（設定、保險庫、外掛、記錄）放進它在
`oryxis.exe` 旁建立的 `data` 資料夾，除非電腦上已有 Oryxis 設定檔；
在 Linux 和 macOS 上，於執行檔、`.AppImage` 檔或 `Oryxis.app` 旁自行
建立 `oryxis-data` 資料夾即可。

### 字型與編碼

首次將介面語言切換為繁體中文時，會自動下載 Noto Sans TC 字型
（按需下載，不會增加安裝程式的體積）。連線舊式裝置（網路設備、
工控主機等）時，可在主機編輯器中為個別主機選擇 Big5 等傳統編碼。

介面的繁體中文翻譯以台灣慣用詞彙撰寫（伺服器、檔案、網路、連接埠），
而不是簡體字的機械轉換。

## 亮點

- **原生且快速**：純 Rust、GPU 加速的 [iced](https://iced.rs) 介面、
  單一二進位檔。沒有 Electron、沒有 webview。
- **本機加密保險庫**：Argon2id + ChaCha20-Poly1305 欄位級加密、可選
  主密碼、生物辨識解鎖（Windows Hello / Touch ID / Linux 金鑰環）、
  閒置自動鎖定、TOTP 兩步驟驗證自動填入，以及在 `sudo` 提示時
  提供保險庫密碼（絕不自動送出）。
- **多個保險庫**：把工作與個人分別放在各自加密的檔案裡，各有自己的
  主密碼、設定、同步與 MCP 開放範圍，可同時開啟：每個視窗使用一個
  保險庫，並可從首頁分頁、各保險庫的快速鍵或命令選擇區就地切換，
  另一個保險庫的分頁照常執行。兩個保險庫絕不共用金鑰。
- **完整的 SSH 能力**：自動驗證、多層跳板機、SOCKS / HTTP / 命令
  代理、Agent 轉發、獨立的 `-L`/`-R`/`-D` 連接埠轉送、面向選單式
  跳板機（JumpServer 等）的 expect/send 登入指令碼。
- **原生支援安全金鑰**：YubiKey 或任何 FIDO2 權杖都能在 Windows、
  Linux 與 macOS 上直接由程式本身簽署 `sk-ssh-ed25519` /
  `sk-ecdsa-sha2-nistp256` 登入（含 Windows Hello），觸碰與 PIN
  都在你的畫面上詢問，另有僅限硬體的「安全金鑰」驗證方式，中間
  不經任何外部代理程式。
- **把你的主機帶過來**：一次匯入就能讀取你手上現有的設定：
  `~/.ssh/config`、PuTTY、KiTTY、WinSCP、mRemoteNG、MobaXterm、
  SecureCRT、Xshell、FinalShell、Termius 或任何 CSV。選好檔案
  （或工作階段資料夾），格式會自動偵測，編輯過的主機 CSV 匯出檔
  會以更新、移除與資料夾的形式匯回。
- **離線由你決定**：一個開關，首次啟動與設定中都有，開啟後
  Oryxis 不會自行發出任何請求：不檢查更新，不下載字型或外掛。
  你自己設定的內容照常運作。給從未連上網路的機器，每個版本都
  附帶一份離線套件，外掛與字型包已內含，開關也已預先開啟。
- **不只 SSH**：Telnet 與序列埠主控台、給主控台伺服器用的純 TCP
  連線、ZMODEM 傳輸、本機 Shell，以及透過 SSH 隧道一鍵開啟
  RDP/VNC。
- **撐得住網路變動的工作階段**：為主機開啟 mosh，Shell 就能撐過
  睡眠、切換 Wi-Fi 與更換位址；介面會直接說明連線已經多久沒有
  聯繫，而不是假裝一切正常。原生 Rust 用戶端，說的是官方
  `mosh-server` 的協定，本機不必額外安裝任何東西。
- **真正的終端機**：以 alacritty 為基礎的模擬器、分割窗格、工作階段
  群組、依主機套用主題、可選的半透明背景或背景圖片、內建 Nerd 字型
  外加可下載字型包（JetBrains Mono、Fira Code、MesloLGS 等）、標示
  長時間執行指令的智慧分頁、依主機保存並在輸入時提供建議的指令歷史、
  形狀由你決定的游標，以及依主機設定的東亞歧義寬度，讓 CJK 文字介面
  對得整齊。
- **依序執行的程式碼片段**：一個程式碼片段就能輸入指令、等待指令
  結束或輸出中出現某一行、複製或儲存它印出的內容，並呼叫其他程式碼
  片段，全部都在你啟動它的那個終端機上進行，變數只詢問一次。最常用的
  還能做成按鈕，放在終端機下方的片段列上。
- **視窗要幾個有幾個**：單一程序、任意數量的視窗，每個視窗都有
  自己的分頁、主機畫面與搜尋。把分頁拖出邊緣即可撕離、放到另一個
  視窗的分頁列即可停靠，或直接把主機連線到新視窗。
- **檔案無所不在**：雙欄 SFTP 支援拖放、符號連結、就地編輯、伺服器對伺服器
  複製；每個 SSH 分頁還有跟隨 Shell 工作目錄的檔案側欄。偏好打字？
  互動式 SFTP 主控台說的是 `sftp(1)` 的指令（`get`、`put`、`mget`、
  `lcd`、萬用字元、Tab 鍵自動完成、行內進度），以你所在工作階段的
  一個窗格開啟（在下方、在旁邊或最大化，隨你選），並以同一個切換鈕
  在終端機、主控台與檔案之間切換。
- **工作階段錄製**：靜態加密儲存；可匯出 asciinema `.cast`（內嵌
  主題）或純文字逐字稿，設計上僅錄製輸出。
- **系統管理員的工具箱**：可選的網路工具面板（預設關閉，以獨立分頁
  開啟）：DNS 記錄、ping、路由追蹤、TCP 連接埠測試、HTTP 重新導向鏈
  與憑證檢查、WHOIS，以及公開的垃圾郵件封鎖清單。
- **雲端帳號**：AWS、Google Cloud、Azure、阿里雲、騰訊雲與 Kubernetes
  的資源探索與連線（EC2、SSM、ECS Exec、GKE、AKS、ACK、TKE、
  `kubectl`），以簽章外掛按需下載。
- **AI 隨侍在側**：每個分頁的 AI 助手（自備金鑰：Anthropic、OpenAI、
  Gemini 或相容服務），多層自動執行安全控管，另有
  [MCP 伺服器](docs/FEATURES.md#mcp-server)可將主機開放給
  Claude Code 等 AI 用戶端，能自行安裝到其中七種用戶端，經你允許後
  還能新增或編輯主機，而以 root 身分執行指令前一定先在畫面上取得你的
  核准。
- **P2P 同步，無雲端**：端對端加密（X25519 + XChaCha20-Poly1305），
  基於 QUIC；區域網路內以 mDNS 探索，跨網路可[自架](SELF_HOSTING.md)
  信令/中繼。沒有帳號，沒有廠商伺服器。
- **鍵盤優先**：`user@host` 快速連線（Ctrl+K）、依最近使用或分頁列
  順序切換的 Ctrl+Tab、涵蓋到最後一個開關的完整鍵盤導覽、所有快速鍵
  皆可重新綁定，也可以從 Windows Terminal、Tabby 或 iTerm2 的按鍵
  開始。
- **隱私至上**：沒有任何遙測、隱私模式遮罩、貼上前讓你確認內容的
  貼上防護，以及含完整 RTL 支援的
  [23 種語言](docs/FEATURES.md#themes--internationalization)：English、
  Português、Español、Français、Deutsch、Italiano、简体中文、繁體中文、
  日本語、Русский、فارسی、العربية、עברית、한국어、Polski、Türkçe、
  Bahasa Indonesia、Tiếng Việt、Українська、ไทย、हिन्दी、Čeština、Ελληνικά。

完整功能清單見英文[功能總覽](docs/FEATURES.md)。
在用 tmux？**[tmux 下的日誌與命令歷史](docs/TMUX.md)**（英文）說明了哪些功能開箱即用、哪些需要自行安裝。
想讓檔案瀏覽器精確跟隨 shell 的目錄？**[跟隨 shell 的目錄](docs/CWD.md)**（英文）提供了程式碼片段。
想把保險庫的副本帶離這台機器，放進雲端資料夾或其他地方？**[備份與存放位置](docs/BACKUP.md)**（英文）說明同步、匯出，以及把檔案送到目的地的工具。

## 快速上手

1. **首次啟動**：設定主密碼，或先跳過（之後可在設定中開啟，並啟用
   生物辨識解鎖）。
2. **新增主機**：點擊 `+ HOST`，或直接輸入 `user@host`（Ctrl+K）
   免儲存連線。從其他 SSH 用戶端過來？一次匯入就能把它儲存的
   工作階段搬過來。
3. **連線**：點擊主機卡片。分割窗格、檔案側欄、SFTP 和程式碼片段
   都只有一個按鍵的距離。
4. **可選擴充**：AI 聊天（設定 > AI）、MCP 伺服器（設定 > 安全性）、
   裝置間 P2P 同步（設定 > 同步）。

有問題？看看 [FAQ](https://github.com/wilsonglasser/oryxis/discussions/66)，
或發起[討論](https://github.com/wilsonglasser/oryxis/discussions)。

## 安全性

所有敏感資料均以欄位級加密儲存（Argon2id + ChaCha20-Poly1305），主機
金鑰採 TOFU 釘選，同步資料端對端加密，外掛在執行前經過 Ed25519 簽章
驗證，而且沒有任何遙測。

完整的安全模型與弱點揭露政策見 [SECURITY.md](SECURITY.md)。請透過
私密管道回報安全弱點。

## 開發藍圖

Oryxis 以大約每週一次的節奏持續發布，功能就緒即上線。最新穩定版為
**v0.23.0**；完整歷史見 [CHANGELOG.md](CHANGELOG.md)，互動式藍圖見
[藍圖討論](https://github.com/wilsonglasser/oryxis/discussions/67)。
自 0.15.0 以來已推出：分割窗格與 SFTP 主控台、離線模式及其離線套件、
阿里雲與騰訊雲、網路工具面板、一鍵部署中繼、原生安全金鑰、單一程序
內的多個視窗、能依序執行一連串動作的程式碼片段、取自其他終端機的
快速鍵預設組、損毀保險庫的重建、多個保險庫、可依分頁列本身順序切換的
Ctrl+Tab、在輸入時提供建議的指令歷史、片段列、游標形狀與閃爍，以及
經畫面核准後透過 MCP 外掛執行的 sudo、帶有 `data` 資料夾的可攜模式、
能維護保險庫的主機 CSV，以及 SFTP 中的符號連結。正在推進的方向是 AI 維運工具組。

## 參與貢獻

歡迎貢獻。**可以直接用中文開 issue 或參與討論**，維護者會閱讀並盡力
回覆；程式碼、commit 訊息與程式註解請使用英文。開發環境、品質門檻與
專案慣例見 [CONTRIBUTING.md](CONTRIBUTING.md)。

## 授權條款

Copyright (C) 2026 Wilson Glasser。依
[AGPL-3.0-or-later](LICENSE) 授權發布：任何人都可以使用、修改與散布
Oryxis，但透過網路提供的修改版本必須以相同授權公開其原始碼。詳見
[NOTICE](NOTICE)。

---

<p align="center">
  以 Rust 打造，獻給以終端機為家的人。
</p>
