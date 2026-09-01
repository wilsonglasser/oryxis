<p align="center">
  <img src="resources/logo_128.png" width="120" alt="Oryxis logo">
</p>

<h1 align="center">Oryxis</h1>

<p align="center">
  전부 Rust로 만든 모던 SSH 클라이언트. 빠르고, 암호화되고, 네이티브.
</p>

<p align="center">
  <a href="README.md">English</a> | <a href="README.zh-CN.md">简体中文</a> | <a href="README.zh-TW.md">繁體中文</a> | <a href="README.ja.md">日本語</a> | 한국어 | <a href="README.fa.md">فارسی</a> | <a href="README.pt-BR.md">Português (BR)</a>
</p>

<p align="center">
  <a href="https://github.com/wilsonglasser/oryxis/releases/latest"><img src="https://img.shields.io/github/v/release/wilsonglasser/oryxis?color=green" alt="Release"></a>
  <img src="https://img.shields.io/badge/platforms-linux%20%7C%20macos%20%7C%20windows-blue" alt="Platforms">
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-AGPL--3.0-blue" alt="License"></a>
  <a href="https://oryxis.app"><img src="https://img.shields.io/badge/website-oryxis.app-3CBBB1" alt="Website"></a>
</p>

<p align="center">
  <img src="resources/screen_1.gif" width="720" alt="Oryxis 시연: 호스트 접속, 스니펫 실행, SFTP 탐색">
</p>

> 이 문서는 v0.15.0 이후의 영어 README를 번역한 것입니다(2026-08-24 동기화).
> 상세 문서([기능 소개](docs/FEATURES.md), [아키텍처](docs/ARCHITECTURE.md))는 영어로 제공됩니다.

## Oryxis란?

Oryxis는 [Termius](https://termius.com/)의 오픈 소스 대안입니다.
모던한 UI와 자격 증명을 보관하는 로컬 암호화 볼트를 갖춘 데스크톱
SSH 클라이언트이며, 그 어디에도 클라우드 계정이 끼어들지 않습니다.
Electron 없음, webview 없음, 벤더 서버 없음. 네이티브 바이너리
하나뿐입니다.

|  | Oryxis | Termius | PuTTY | Tabby |
|--|--------|---------|-------|-------|
| UI 스택 | 네이티브 Rust(iced + wgpu) | Electron | 네이티브 | Electron |
| 라이선스 | AGPL-3.0, 오픈 소스 | 독점 | MIT | MIT |
| 자격 증명 저장 | 로컬 암호화 볼트 | 벤더 클라우드 계정 | 없음 | 로컬 설정 파일 |
| 기기 동기화 | P2P 종단간 암호화, 셀프 호스팅 릴레이 지원 | 벤더 클라우드(구독) | 없음 | Tabby Web 경유 |
| SFTP GUI | 듀얼 패널 내장 | 유료 플랜 | CLI만 | 기본 패널 |
| 가격 | 무료 | 무료 티어 + 구독 | 무료 | 무료 |

## 설치

**Windows**

[![Microsoft Store에서 다운로드](https://get.microsoft.com/images/ko%20dark.svg)](https://apps.microsoft.com/detail/9NTKPPSHBTG2)

또는 터미널에서:

```powershell
winget install WilsonGlasser.Oryxis
```

**Arch Linux (AUR)**

```bash
yay -S oryxis-bin
```

**직접 다운로드**: [최신 릴리스](https://github.com/wilsonglasser/oryxis/releases/latest)에서
Linux(`.tar.gz` / `.deb` / `.AppImage`, x86_64 및 ARM64),
macOS(Apple Silicon `.dmg`), Windows(시스템 / 사용자 단위 설치
프로그램과 포터블 `.zip`, x86_64 및 ARM64)를 제공합니다. Windows
바이너리는 Authenticode 서명이 되어 있습니다.

### 글꼴과 인코딩

UI 언어를 한국어로 바꾸면 Noto Sans KR 글꼴이 자동으로 다운로드
됩니다(온디맨드 방식이라 설치 파일 크기에는 영향이 없습니다).
네트워크 장비 같은 레거시 기기에 접속할 때는 호스트별로 EUC-KR
같은 인코딩을 선택할 수 있습니다.

## 하이라이트

- **네이티브이고 빠릅니다.** 순수 Rust, GPU 가속
  [iced](https://iced.rs) UI, 단일 바이너리. Electron도 webview도
  없습니다.
- **로컬 암호화 볼트.** Argon2id + ChaCha20-Poly1305 필드 단위
  암호화, 선택형 마스터 비밀번호, 생체 인식 잠금 해제(Windows
  Hello / Touch ID / Linux 키링), 유휴 자동 잠금, 2FA 호스트용
  TOTP 자동 입력, `sudo` 비밀번호 프롬프트에서 볼트 비밀번호 제안
  (자동 전송은 하지 않습니다).
- **완전한 SSH 파이프라인.** 자동 인증, 다단계 점프 호스트,
  SOCKS / HTTP / 명령 프록시, 에이전트 포워딩, 독립형
  `-L`/`-R`/`-D` 포트 포워딩, 메뉴형 배스천(JumpServer 등)을 위한
  expect/send 로그인 스크립트, `~/.ssh/config` 원클릭 가져오기.
- **SSH만이 아닙니다.** Telnet과 시리얼 콘솔, 콘솔 서버를 위한 순수
  TCP 연결, ZMODEM 전송, 로컬 셸, SSH 터널을 통한 원클릭 RDP/VNC.
- **네트워크가 바뀌어도 이어지는 세션.** 호스트에 mosh를 켜면 셸이
  절전, Wi-Fi 전환, 주소 변경을 모두 견딥니다. 게다가 아무 일도
  없는 척하지 않고, 링크가 얼마나 오래 끊겨 있었는지를 화면이
  알려 줍니다. 정품 `mosh-server`의 프로토콜을 쓰는 네이티브 Rust
  클라이언트라, 내 컴퓨터에 따로 설치할 것은 없습니다.
- **진짜 터미널.** alacritty 기반 에뮬레이터, 분할 창, 세션 그룹,
  호스트별 테마, 번들 Nerd Fonts와 내려받는 폰트 팩(JetBrains
  Mono, Fira Code, MesloLGS 등), 오래 걸리는 명령을 알려 주는
  스마트 탭, 호스트별 명령 히스토리.
- **어디서나 파일.** 드래그 앤 드롭, 즉석 편집, 서버 간 복사를
  지원하는 듀얼 패널 SFTP. 모든 SSH 탭에는 셸의 작업 디렉터리를
  따라가는 파일 사이드바도 있습니다.
- **세션 녹화.** 저장 시 암호화. asciinema `.cast`(테마 포함) 또는
  일반 텍스트로 내보내기. 설계상 출력만 기록합니다.
- **클라우드 계정.** AWS, Google Cloud, Azure, Kubernetes 리소스
  검색과 연결(EC2, SSM, ECS Exec, GKE, AKS, `kubectl`). 서명된
  플러그인으로 필요할 때만 다운로드됩니다.
- **일하는 곳에 AI를.** 탭별 어시스턴트(자체 API 키 사용:
  Anthropic, OpenAI, Gemini 또는 호환 API), 다층 자동 실행
  안전장치, 그리고 [MCP 서버](docs/FEATURES.md#mcp-server)로
  Claude Code 같은 AI 클라이언트에 호스트를 노출할 수 있습니다.
- **클라우드 없는 P2P 동기화.** QUIC 위의 종단간 암호화(X25519 +
  XChaCha20-Poly1305). LAN에서는 mDNS, 네트워크를 넘어갈 때는
  [셀프 호스팅](SELF_HOSTING.md) 시그널링 / 릴레이. 계정도 벤더
  서버도 없습니다.
- **키보드 우선.** `user@host` 빠른 연결(Ctrl+K), MRU 탭 전환,
  마지막 토글 하나까지 닿는 완전한 키보드 내비게이션, 모든 단축키
  재지정 가능.
- **설계부터 프라이버시.** 텔레메트리 전무, 프라이버시 모드
  마스킹, 붙여넣을 내용을 먼저 보여 주는 붙여넣기 가드, 완전한
  RTL 지원을 포함한 [23개 언어](docs/FEATURES.md#themes--internationalization):
  English, Português, Español, Français, Deutsch, Italiano, 简体中文,
  繁體中文, 日本語, Русский, فارسی, العربية, עברית, 한국어, Polski,
  Türkçe, Bahasa Indonesia, Tiếng Việt, Українська, ไทย, हिन्दी,
  Čeština, Ελληνικά.

전체 기능 목록은 영어 [기능 소개](docs/FEATURES.md)에 있습니다.
tmux를 사용하시나요? **[tmux에서의 로그와 명령 기록](docs/TMUX.md)**(영어)이 기본으로 동작하는 것과 직접 설치해야 하는 것을 설명합니다.
파일 브라우저가 셸의 디렉터리를 정확히 따라가게 하려면 **[셸 디렉터리 따라가기](docs/CWD.md)**(영어)에 스니펫이 있습니다.

## 빠른 시작

1. **첫 실행:** 마스터 비밀번호를 설정하거나 일단 건너뜁니다
   (나중에 설정에서 생체 인식 잠금 해제와 함께 켤 수 있습니다).
2. **호스트 추가:** `+ HOST`를 클릭하거나 `user@host`를 입력
   (Ctrl+K)해 저장 없이 바로 접속합니다. `~/.ssh/config`는
   원클릭으로 가져옵니다.
3. **접속:** 호스트 카드를 클릭합니다. 분할 창, 파일 사이드바,
   SFTP, 스니펫이 키 하나 거리에 있습니다.
4. **선택 사항:** AI 채팅(설정 > AI), MCP 서버(설정 > 보안),
   기기 간 P2P 동기화(설정 > 동기화).

궁금한 점은 [FAQ](https://github.com/wilsonglasser/oryxis/discussions/66)를
보거나 [Discussion](https://github.com/wilsonglasser/oryxis/discussions)을
열어 주세요.

## 보안

모든 민감한 데이터는 필드 단위로 암호화되어 저장되고(Argon2id +
ChaCha20-Poly1305), 호스트 키는 TOFU로 고정되며, 동기화 페이로드는
종단간 암호화되고, 플러그인은 실행 전에 Ed25519 서명을 검증합니다.
텔레메트리는 일절 없습니다.

전체 보안 모델과 취약점 공개 정책은 [SECURITY.md](SECURITY.md)에
있습니다. 취약점은 비공개 경로로 제보해 주세요.

## 로드맵

Oryxis는 대략 매주 작은 단위로 릴리스하며, 기능은 준비되는 대로
출시됩니다. 최신 안정 버전은 **v0.15.0**입니다. 전체 이력은
[CHANGELOG.md](CHANGELOG.md), 인터랙티브 로드맵은
[로드맵 Discussion](https://github.com/wilsonglasser/oryxis/discussions/67)에서
볼 수 있습니다. 진행 중인 방향: 네이티브 FIDO2(USB / NFC로 보안
키와 직접 통신), 다중 볼트. 네이티브 mosh 클라이언트, 호스트별
디스크 키(`~/.ssh`), 직전에 닫은 탭 되살리기는 이번 버전에
출시되었습니다. 동아시아 모호 폭(ambiguous width) 옵션은 다음
버전에 포함됩니다.

## 기여하기

기여를 환영합니다. 한국어로 issue나 Discussion을 열어도 됩니다
(메인테이너는 번역기를 활용해 읽습니다). 코드, 커밋 메시지, 주석은
영어를 사용해 주세요. 개발 환경, 품질 게이트, 프로젝트 규약은
[CONTRIBUTING.md](CONTRIBUTING.md)를 참고하세요.

## 라이선스

Copyright (C) 2026 Wilson Glasser.
[AGPL-3.0-or-later](LICENSE) 라이선스로 배포됩니다. 누구나
Oryxis를 사용, 수정, 배포할 수 있지만, 수정한 버전을 네트워크로
제공하는 경우 동일한 라이선스로 소스 코드를 공개해야 합니다.
자세한 내용은 [NOTICE](NOTICE)를 참고하세요.

---

<p align="center">
  터미널에서 사는 사람들을 위해, Rust로 만들었습니다.
</p>
