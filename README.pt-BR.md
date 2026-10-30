<p align="center">
  <img src="resources/logo_128.png" width="120" alt="Logo do Oryxis">
</p>

<h1 align="center">Oryxis</h1>

<p align="center">
  Um cliente SSH moderno construído inteiramente em Rust. Rápido, criptografado, nativo.
</p>

<p align="center">
  <a href="README.md">English</a> | <a href="README.zh-CN.md">简体中文</a> | <a href="README.zh-TW.md">繁體中文</a> | <a href="README.ja.md">日本語</a> | <a href="README.ko.md">한국어</a> | <a href="README.fa.md">فارسی</a> | Português (BR)
</p>

<p align="center">
  <a href="https://github.com/wilsonglasser/oryxis/releases/latest"><img src="https://img.shields.io/github/v/release/wilsonglasser/oryxis?color=green" alt="Release"></a>
  <img src="https://img.shields.io/badge/platforms-linux%20%7C%20macos%20%7C%20windows-blue" alt="Platforms">
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-AGPL--3.0-blue" alt="License"></a>
  <a href="https://oryxis.app"><img src="https://img.shields.io/badge/website-oryxis.app-3CBBB1" alt="Website"></a>
</p>

<p align="center">
  <img src="resources/screen_1.gif" width="720" alt="Oryxis em ação: conectando, executando snippets, navegando por SFTP">
</p>

> Este documento é uma tradução do README em inglês posterior à v0.23.0
> (sincronizado em 2026-10-29). A documentação detalhada
> ([tour de funcionalidades](docs/FEATURES.md),
> [arquitetura](docs/ARCHITECTURE.md)) permanece em inglês.

## O que é o Oryxis?

O Oryxis é uma alternativa open source ao [Termius](https://termius.com/):
um cliente SSH de desktop com interface moderna e um cofre local
criptografado para credenciais, sem nenhuma conta na nuvem em lugar
algum. Sem Electron, sem webview, sem servidores de terceiros. Só um
binário nativo.

|  | Oryxis | Termius | PuTTY | Tabby |
|--|--------|---------|-------|-------|
| Stack de UI | Rust nativo (iced + wgpu) | Electron | Nativo | Electron |
| Licença | AGPL-3.0, open source | Proprietária | MIT | MIT |
| Armazenamento de credenciais | Cofre local criptografado | Conta na nuvem do fornecedor | Nenhum | Arquivos de config locais |
| Sincronização entre dispositivos | P2P com criptografia de ponta a ponta, relay auto-hospedado opcional | Nuvem do fornecedor (assinatura) | Nenhuma | Via Tabby Web |
| SFTP | GUI de painel duplo **e** um console interativo | Plano pago | Só CLI | Painel básico |
| Preço | Grátis | Plano grátis + assinatura | Grátis | Grátis |

## Instalação

**Windows**

[![Baixar na Microsoft Store](https://get.microsoft.com/images/pt-br%20dark.svg)](https://apps.microsoft.com/detail/9NTKPPSHBTG2)

ou, pelo terminal:

```powershell
winget install WilsonGlasser.Oryxis
```

**Arch Linux (AUR)**

```bash
yay -S oryxis-bin
```

**Downloads diretos**: na [página da última versão](https://github.com/wilsonglasser/oryxis/releases/latest),
com builds para Linux (`.tar.gz` / `.deb` / `.AppImage`, x86_64 e
ARM64), macOS (`.dmg` para Apple Silicon) e Windows (instaladores de
sistema e por usuário, além do `.zip` portátil, x86_64 e ARM64). Os
binários de Windows são assinados com Authenticode. O `.zip` portátil
guarda tudo (configurações, cofres, plugins, logs) numa pasta `data`
que ele cria ao lado do `oryxis.exe`, a menos que o computador já
tenha um perfil do Oryxis; no Linux e no macOS, crie uma pasta `oryxis-data`
ao lado do binário, do arquivo `.AppImage` ou do `Oryxis.app` para o
mesmo efeito.

## Destaques

- **Nativo e rápido**: Rust puro, interface [iced](https://iced.rs)
  acelerada por GPU, binário único. Sem Electron, sem webview.
- **Cofre local criptografado**: Argon2id + ChaCha20-Poly1305 campo a
  campo, senha mestra opcional, desbloqueio biométrico (Windows Hello /
  Touch ID / keyring do Linux), bloqueio automático por inatividade e
  preenchimento automático de TOTP para hosts com 2FA e senhas do
  cofre oferecidas nos prompts de `sudo` (nunca enviadas sozinhas).
- **Vários cofres**: Trabalho e Pessoal em arquivos criptografados
  separados, cada um com sua senha mestra, configurações, sincronização
  e exposição ao MCP, abertos ao mesmo tempo: cada janela trabalha num
  cofre e troca na hora pela aba Início, por um atalho por cofre ou pela
  paleta, enquanto as abas do outro cofre continuam rodando. Dois cofres
  nunca compartilham uma chave.
- **O pipeline SSH completo**: autenticação automática, jump hosts em
  cadeia, proxies SOCKS / HTTP / de comando, encaminhamento de agente,
  port forwarding independente `-L`/`-R`/`-D` e scripts de login
  expect/send para bastions de menu (JumpServer e companhia).
- **Chaves de segurança, nativas**: uma YubiKey ou qualquer token FIDO2
  assina logins `sk-ssh-ed25519` / `sk-ecdsa-sha2-nistp256` a partir do
  próprio app no Windows, Linux e macOS (Windows Hello incluído), com o
  toque e o PIN pedidos na sua tela, um método Chave de Segurança só por
  hardware e nenhum agente externo no meio.
- **Traga seus hosts junto**: uma importação lê o que você já tem:
  `~/.ssh/config`, PuTTY, KiTTY, WinSCP, mRemoteNG, MobaXterm,
  SecureCRT, Xshell, FinalShell, Termius ou qualquer CSV. Escolha o
  arquivo (ou a pasta de sessões) e o formato é detectado para você;
  um CSV de hosts exportado e editado volta como atualizações,
  remoções e pastas.
- **Offline quando você mandar**: um interruptor, oferecido na primeira
  execução e nas Configurações, e o Oryxis não faz nenhuma requisição
  por conta própria: sem checagem de atualização, sem download de fontes
  ou plugins. O que você mesmo configurou continua funcionando. Para uma
  máquina que nunca teve rede, um bundle offline sai ao lado de cada
  release com os plugins e os pacotes de fontes já dentro e o
  interruptor já ligado.
- **Mais que SSH**: consoles Telnet e serial para os equipamentos que
  nunca aprenderam SSH, linhas TCP puras para servidores de console,
  transferências ZMODEM, shells locais e RDP/VNC em um clique através
  de túnel SSH.
- **Sessões que sobrevivem à rede**: ligue o mosh num host e o shell
  atravessa a suspensão, a troca de Wi-Fi e a troca de endereço, com a
  interface dizendo há quanto tempo o link está sem contato em vez de
  fingir que está tudo bem. Cliente nativo em Rust falando o protocolo
  do `mosh-server` original, então não há nada a instalar na sua
  máquina.
- **Um terminal de verdade**: emulador baseado no alacritty, painéis
  divididos, grupos de sessão, temas por host, fundo translúcido ou
  imagem de fundo opcionais, Nerd Fonts embutidas mais um pacote de
  fontes baixáveis (JetBrains Mono, Fira Code, MesloLGS e outras), abas
  inteligentes que sinalizam comandos demorados, histórico de comandos
  por host sugerido enquanto você digita, um cursor no formato que você
  escolher e um ajuste por host da largura ambígua do leste asiático
  para as TUIs CJK alinharem.
- **Snippets que rodam uma sequência**: um snippet pode digitar
  comandos, esperar um comando terminar ou uma linha aparecer na saída,
  copiar ou salvar o que ele imprimiu e chamar outros snippets, tudo no
  terminal em que você o iniciou, com as variáveis perguntadas uma vez.
  Os que você mais usa podem virar botões numa barra de snippets abaixo
  do terminal.
- **Quantas janelas quiser**: um processo, qualquer número de janelas,
  cada uma com suas abas, sua tela de hosts e sua busca. Arraste uma aba
  para fora da borda para destacá-la, solte-a na barra de abas de outra
  janela para encaixá-la, ou conecte um host direto numa janela nova.
- **Arquivos em todo lugar**: SFTP de painel duplo com arrastar e
  soltar, links simbólicos, edição no lugar e cópia servidor a
  servidor; toda aba SSH ainda traz uma barra lateral de arquivos que segue o diretório do
  shell. Prefere digitar? Um console SFTP interativo fala os comandos
  do `sftp(1)` (`get`, `put`, `mget`, `lcd`, globs, completação com
  Tab, progresso em linha), abrindo como um painel da sessão em que
  você já está (empilhado, ao lado ou ampliado, à sua escolha) com um
  único interruptor entre terminal, console e arquivos.
- **Gravação de sessões**: criptografada em repouso; exporta para
  asciinema `.cast` (com tema embutido) ou transcrição em texto puro,
  gravando apenas a saída por decisão de projeto.
- **A caixa de ferramentas do sysadmin**: um painel opcional de
  ferramentas de rede (desligado por padrão, abre como aba própria):
  registros DNS, ping, traceroute, teste de porta TCP, cadeia de
  redirecionamentos HTTP e inspeção de certificado, WHOIS e as listas
  públicas de bloqueio de spam.
- **Contas de nuvem**: descoberta e conexão em AWS, Google Cloud,
  Azure, Alibaba Cloud, Tencent Cloud e Kubernetes (EC2, SSM, ECS Exec,
  GKE, AKS, ACK, TKE, `kubectl`), distribuídas como plugins assinados
  baixados sob demanda.
- **IA onde você trabalha**: assistente por aba (com a sua própria
  chave: Anthropic, OpenAI, Gemini ou compatível) com camadas de
  segurança para execução automática, além de um
  [servidor MCP](docs/FEATURES.md#mcp-server) que expõe seus hosts a
  clientes de IA como o Claude Code, se instala em sete deles, cria ou
  edita hosts quando você permite e roda um comando como root só depois
  que você aprova na tela.
- **Sincronização P2P, sem nuvem**: criptografia de ponta a ponta
  (X25519 + XChaCha20-Poly1305) sobre QUIC; mDNS na rede local e
  signaling/relay [auto-hospedado](SELF_HOSTING.md) entre redes. Sem
  conta, sem servidor de fornecedor.
- **Teclado em primeiro lugar**: conexão rápida `user@host` (Ctrl+K),
  Ctrl+Tab por uso recente ou pela ordem da barra de abas, navegação
  completa por teclado até o último toggle e todos os atalhos
  reconfiguráveis, ou comece pelas
  teclas do Windows Terminal, do Tabby ou do iTerm2.
- **Privado por padrão**: zero telemetria, mascaramento com Modo
  Privacidade, uma proteção de colagem que mostra o que você está
  colando e [23 idiomas](docs/FEATURES.md#themes--internationalization)
  com suporte completo a RTL: English, Português, Español, Français,
  Deutsch, Italiano, 简体中文, 繁體中文, 日本語, Русский, فارسی, العربية,
  עברית, 한국어, Polski, Türkçe, Bahasa Indonesia, Tiếng Việt,
  Українська, ไทย, हिन्दी, Čeština, Ελληνικά.

O inventário completo está no
[tour de funcionalidades](docs/FEATURES.md) (em inglês).
Usa tmux? **[Logs e histórico de comandos no tmux](docs/TMUX.md)** (em
inglês) explica o que funciona de fábrica e o que você mesmo instala.
Quer o navegador de arquivos seguindo o shell com exatidão?
**[Seguindo o diretório do shell](docs/CWD.md)** (em inglês) tem o snippet.
Quer levar uma cópia do seu cofre para fora desta máquina, para uma
pasta na nuvem ou qualquer outro lugar? **[Backups e onde
guardá-los](docs/BACKUP.md)** (em inglês) cobre sincronização,
exportação e as ferramentas que levam um arquivo o resto do caminho.

## Primeiros passos

1. **Primeira execução**: escolha uma senha mestra ou continue sem uma
   (dá para ativar depois nas Configurações, junto com o desbloqueio
   biométrico).
2. **Adicione hosts**: clique em `+ HOST`, ou só digite `user@host`
   (Ctrl+K) para conectar sem salvar. Vem de outro cliente SSH? A
   importação traz as sessões salvas dele em um passo.
3. **Conecte**: clique no card do host. Painéis divididos, barra de
   arquivos, SFTP e snippets ficam a uma tecla de distância.
4. **Extras opcionais**: chat de IA (Configurações > IA), servidor MCP
   (Configurações > Segurança), sincronização P2P entre seus
   dispositivos (Configurações > Sincronização).

Dúvidas? Veja o
[FAQ](https://github.com/wilsonglasser/oryxis/discussions/66) ou abra
uma [Discussão](https://github.com/wilsonglasser/oryxis/discussions).

## Segurança

Tudo que é sensível é criptografado campo a campo em repouso
(Argon2id + ChaCha20-Poly1305), as chaves de host são fixadas via
TOFU, os dados de sincronização têm criptografia de ponta a ponta, os
plugins são verificados por assinatura Ed25519 antes de executar, e
não existe telemetria de espécie alguma.

O modelo de segurança completo e a política de divulgação de
vulnerabilidades estão em [SECURITY.md](SECURITY.md). Reporte
vulnerabilidades por canal privado.

## Roadmap

O Oryxis lança pequeno e com frequência (aproximadamente semanal), e
as funcionalidades entram assim que ficam prontas. A última versão
estável é a **v0.23.0**; o histórico completo está no
[CHANGELOG.md](CHANGELOG.md) e o roadmap interativo na
[discussão de roadmap](https://github.com/wilsonglasser/oryxis/discussions/67).
Entregues desde a 0.15.0: painéis divididos e o console SFTP, o modo
offline com seu bundle, Alibaba Cloud e Tencent Cloud, o painel de
ferramentas de rede, o deploy do relay em um clique, chaves de
segurança nativas, várias janelas num só processo, snippets que rodam
uma sequência de ações, predefinições de atalhos de outros terminais,
a reconstrução de um cofre danificado, vários cofres abertos ao mesmo
tempo, o Ctrl+Tab na ordem da própria barra de abas, o histórico de
comandos sugerido enquanto você digita, a barra de snippets, o formato
do cursor e o `sudo` pelo plugin MCP com aprovação na tela, o modo portátil
com sua pasta `data`, o CSV de hosts que mantém o cofre e os links
simbólicos no SFTP. Em andamento: o kit de operações com IA.

## Contribuindo

Contribuições são bem-vindas. **Pode abrir issue ou participar das
discussões em português**; código, mensagens de commit e comentários
ficam em inglês. O setup de desenvolvimento, os critérios de qualidade
e as convenções do projeto estão em [CONTRIBUTING.md](CONTRIBUTING.md).

## Licença

Copyright (C) 2026 Wilson Glasser. Licenciado sob
[AGPL-3.0-or-later](LICENSE): qualquer pessoa pode usar, modificar e
distribuir o Oryxis, mas qualquer versão modificada disponibilizada
pela rede precisa publicar seu código-fonte sob a mesma licença.
Detalhes em [NOTICE](NOTICE).

---

<p align="center">
  Feito em Rust, para quem vive no terminal.
</p>
