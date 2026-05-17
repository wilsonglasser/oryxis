# Oryxis v0.7 — UI Refresh + Quality of Life

> Status: planejamento. Este doc consolida o escopo da v0.7 focado em
> rework de UI inspirado no modelo Termius/JetBrains (3 zonas: top tabs
> + sidebar contextual + burger menu), customização visual diferenciadora,
> bugs de produção reportados em issue #18 e o trabalho de fontes (Nerd
> Font bundled + enumeração de fontes do sistema). Os itens originalmente
> previstos pra v0.7 no roadmap (EKS estável, GCP Compute + IAP) são
> remanejados pra v0.8 e v0.9.

## Resumo

| Versão | Escopo |
|--------|--------|
| **v0.6** (released) | AWS provider (EC2 + ECS) + Kubernetes provider (experimental) + per-host initial command |
| **v0.7** (este doc) | UI rework (Workspace layout), Interface settings section, accent dinâmico, host icons customizáveis, burger menu, Nerd Font bundled, bugs de UX |
| **v0.8** | EKS estabilizado, port forward como entidade independente, split panes (era v0.6) |
| **v0.9** | GCP Compute + IAP, biometric unlock, custom themes |
| **v1.0** | Azure, system tray, polish geral |

## Motivação

Issue #18 (koobs) trouxe feedback de primeiro contato:

- Bugs concretos (right-click paste não funciona em SSH, botão AI Chat
  não respeita o setting de enable, ausência de Solarized Dark).
- Pedidos de minimalismo (esconder sidebar, status bar opcional,
  burger menu).
- Customização visual (font fallback pra glyphs Unicode, posição do
  close button na tab).

A resposta natural é uma **release dedicada a quality-of-life**.
Aproveitamos a janela pra um rework de UI maior, inspirado nas 3 zonas
do Termius (top tabs + sidebar contextual + burger), mas com identidade
visual própria pra reduzir risco de trade dress (ver seção
"Diferenciação visual").

## Visão arquitetural

### 2 Layout modes

A escolha mora em Interface settings, **default = Workspace pra todos**
(inclusive instalações existentes). Quem preferir, troca em 1 clique.

**Workspace mode (novo default)**

Inspirado em Termius (que por sua vez espelha VSCode + browser):

- **Top tab bar** unificada: `[O] [☰]  [⊙ Vault ▾] [SFTP]  [● SP-Mundi-API ×] [SIS-NAT ×] [+]`
  - `[O]` logo Oryxis pequeno top-left (estilo JetBrains, ícone do produto)
  - `[☰]` burger menu com Settings/Updates/About/Local Terminal/Exit
  - `[⊙ Vault ▾]` e `[SFTP]` são áreas top-level (sempre acessíveis)
  - `[● Host ×]` cada conexão aberta é uma tab; `●` é status dot colorido
- **Sidebar contextual**: só aparece quando a área ativa é `Vault`.
  Lista: Hosts (default) · Keychain · Snippets · Known Hosts · (Port Forwarding em v0.8) · Logs
- **Terminal full-canvas**: ao abrir uma conexão, sidebar some, só a
  top tab bar fica. Maximiza espaço útil sem hack.

**Classic mode (legacy, opt-in)**

Sidebar global esquerda fixa, comportamento atual preservado pra quem
já se acostumou. Botão de collapse separado dos itens (vai pra borda /
footer da sidebar, não fica misturado com Hosts/Keychain). Quando
colapsada continua mostrando ícones (56px), como hoje.

### Accent dinâmico (JetBrains-style)

Cada `Connection` ganha campo opcional `accent_color: Option<Color>`.
Quando a tab está ativa:

- Border-bottom 1-2px do header puxa essa cor (gradient sutil, alpha baixa).
- Tab ativa tem indicador colorido.
- AI Chat sidebar header puxa um tint dessa cor.
- Animação suave (~200ms) ao trocar de tab.

Resultado: usuário sabe "ah, o laranja é prod, o azul é dev" sem
precisar ler o nome da tab. Mesmo efeito do JetBrains quando você
troca de projeto: o IDE "respira" a cor do projeto.

Fallback: se `accent_color` for `None`, usa o accent global Oryxis.

### Host icons customizáveis

Hoje o ícone do host é um quadrado arredondado preenchido com cor
sólida + glyph centralizado. **Idêntico ao Termius visualmente** — é
o ponto fraco de trade dress.

**Default novo:** circular com glyph centralizado.

**Opções configuráveis (global + per-host override):**

- `Circular` — anel preenchido (default)
- `Square` — preenchido (estilo legado / Termius-like)
- `Outline` — borda colorida + fundo transparente + glyph cor da borda
- `Initials` — letra dupla colorida, sem fundo (estilo GitHub repos)
- `Custom emoji` — usuário cola emoji (🦊, 🚀, etc)

Customização vira **diferencial real** do Oryxis (Termius não permite).
O campo `Color` per-host vira a fonte do `accent_color` automaticamente.

## Modelo de dados

### `Connection` ganha campos

```rust
pub struct Connection {
    // ...campos existentes...
    pub icon_style: Option<HostIconStyle>,   // None = use global default
    pub icon_color: Option<Color>,            // None = derived from cloud provider / hash
    pub accent_color: Option<Color>,          // None = use global accent
}

pub enum HostIconStyle {
    Circular,
    Square,
    Outline,
    Initials,
    Emoji(String),  // 1-2 char string
}
```

Migration: campos novos com `#[serde(default)]`, `None` em hosts
existentes (preserva comportamento). Coluna no SQLite: `icon_config
TEXT` (JSON serialized) ou colunas separadas. Decisão durante PR.

### `View` enum vira 2-níveis

```rust
pub enum View {
    Vault(VaultSection),  // Hosts / Keychain / Snippets / KnownHosts / Logs
    Sftp,
    Settings(SettingsSection),
    Connection(usize),    // tab index
}

pub enum VaultSection {
    Hosts,
    Keychain,
    Snippets,
    KnownHosts,
    Logs,
}
```

Hoje cada item da sidebar é um modo "flat" do app. Agrupar sob `Vault`
abre espaço pra sidebar contextual.

## Settings novos

| Key | Tipo | Default | Descrição |
|-----|------|---------|-----------|
| `layout_mode` | string | `"workspace"` | `"workspace"` ou `"classic"` |
| `show_status_bar` | bool | `true` | Esconde a status bar inferior |
| `show_tab_status_dot` | bool | `true` | Indicador colorido de conexão na tab |
| `tab_close_button_side` | string | `"right"` | `"left"` ou `"right"` |
| `default_host_icon_style` | string | `"circular"` | Tipo de ícone default |
| `bundled_font_fallback` | bool | `true` | Usa Nerd Font bundled como fallback |

`enable_sftp` (toggle pra esconder SFTP do sidebar) **não é necessário**
no Workspace mode (SFTP é área top-level, sempre acessível). Permanece
útil só no Classic mode; vamos adicionar mesmo assim por simetria com
`ai_enabled`.

## Settings reorganizadas: nova section `Interface`

A section `Theme` atual é **absorvida**. Layout da nova section:

```
Interface
├── App theme (cards)
├── Terminal theme (cards)
├── Language + Layout direction (RTL/LTR/Auto)
├── Layout mode: ◉ Workspace  ○ Classic
├── Navigation
│   └── Default host icon style: ▼ Circular / Square / Outline / Initials
├── Tabs
│   ├── Close button position: ▼ Left / Right
│   └── [x] Show connection status dot
└── Status bar
    └── [x] Show status bar
```

Mantém em outras sections:
- **Terminal**: font, font size, scrollback, copy-on-select, etc.
- **AI**: provider, model, key, `[x] Enable AI`
- **SFTP**: `[x] Enable SFTP` (novo, simetria com AI)

Section `Theme` no enum `SettingsSection` é removida (não deprecated —
boot.rs mapeia stale persisted value pra `Interface`).

## Bugs corrigidos

### Right-click paste em SSH (issue #18)

`widget.rs:1100` faz `state.write(text.as_bytes())` no right-click, mas
isso só atinge o PTY local. Sessão SSH vive em `tab.active().ssh_session`
e precisa de `ssh.write()` (vide `dispatch_terminal.rs:158` que faz
isso pro Ctrl+Shift+V).

Fix: adicionar callback `on_paste(msg_fn)` em TerminalView, emitir o
texto da clipboard como Message do app. Dispatcher rota pra
`ssh_session.write()` ou `terminal.lock().write()` igual o Ctrl+Shift+V.

### Botão AI Chat com `ai_enabled = false` (issue #18)

`views/terminal.rs:48` renderiza o toggle de chat checando só
`chat_visible`. Adicionar guard `if chat_visible || !self.ai_enabled`.

### Falta Solarized Dark como AppTheme (issue #18)

A palette `solarized_dark` existe em `oryxis-terminal/src/colors.rs:172`
mas não tem `AppTheme::SolarizedDark` equivalente. Adicionar variant,
ALL entry, name(), from_name(), match em `OryxisColors::t()`, e criar
const `SOLARIZED_DARK: ThemeColors` espelhando o padrão do
`SOLARIZED_LIGHT`.

## Fontes

### Bundle Nerd Font como fallback

Adicionar JetBrains Mono Nerd Font ao binário (~2MB). Carregar como
fonte secundária no fontdb da boot. cosmic-text usa per-glyph fallback
automaticamente quando a fonte primária não cobre um glyph.

Setting `bundled_font_fallback: bool` (default true) permite o usuário
desabilitar se quiser binário menor / fallback puramente do sistema.

### Enumerar fontes mono do sistema

Substituir o array hardcoded `TERMINAL_FONTS` em `app.rs:49` por
enumeração via `fontdb`. Filtrar fonts monospace (`is_monospace` flag).

Fallback: se enumeração falhar ou retornar lista vazia, usa o array
hardcoded como safety net.

### Auditoria

Validar com cenário real (Box Drawing + Powerline + emoji) que o
fallback per-glyph do cosmic-text está funcionando no terminal widget.
Se não estiver, investigar se o widget está usando `text!()` macro ou
renderização manual de glyph que bypassa o fallback.

## Diferenciação visual (juridicamente defensiva)

Análise de risco trade dress / look-and-feel vs Termius:

- **Patterns são livres**: sidebar à esquerda, cards de host, top tabs,
  burger menu — todos universais. Apple v. Microsoft (1994) matou
  "look and feel" como protegível por copyright nos EUA. Lei BR
  9.609/98 cobre expressão não funcional.
- **Risco real estava na estética distintiva**: ícones quadrados
  coloridos com mesma paleta e proporção do Termius. Card layout
  quase 1:1.

**Mitigações na v0.7:**

1. **Ícones circulares como default** (Termius usa quadrados).
2. **Paleta de cores customizável per-host** (Termius fixa).
3. **Accent dinâmico** no chrome (Termius não tem).
4. **Sync indicator** sempre visível (Termius é cloud-only, não tem).
5. **Logo Oryxis** persistente top-left (Termius esconde após login).
6. **README/About** menciona inspiração explicitamente:
   > "Oryxis takes UX inspiration from Termius and other modern SSH
   > clients, rebuilt as open-source, privacy-first with local storage,
   > P2P sync, and no required cloud account."

Posicionamento honesto + diferenças funcionais + customização que eles
não têm = praticamente imune a passing off / concorrência desleal.

## UX

### Header novo (Workspace mode)

```
┌────────────────────────────────────────────────────────────────────────┐
│ [O] [☰] │ [⊙Vault▾] [SFTP]│ [● SP-Mundi-API ×] [SIS-NAT ×] [+]│ ─ □ × │
└─━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┘
  ↑ logo  ↑ burger  ↑ áreas        ↑ tabs de conexão              ↑ window
                                    (border-bottom = accent do host ativo)
```

### Burger menu (novo)

```
[☰]
  Vault                   Ctrl+1
  SFTP                    Ctrl+2
  Settings                Ctrl+,
  ─────────────
  New Local Terminal
  New Serial Connection   (placeholder, item v0.8)
  ─────────────
  Sync Status…
  Check for Updates…
  About / Help
  ─────────────
  Exit
```

Funciona em ambos os layout modes. No Classic mode, é só uma forma
extra de acessar settings/updates (sidebar permanece a navegação
primária).

### Sidebar contextual (Workspace mode, área Vault)

Lista vertical, items condicionais por capability:

```
┌────────────┐
│ Hosts       │ ← default
│ Keychain    │
│ Snippets    │
│ Known Hosts │
│ Logs        │
└────────────┘
```

`Cloud Accounts` permanece em Settings (não vira sidebar item).
`Identities` / `Proxies` / `SSH Keys` se consolidam dentro de
`Keychain` como sub-tabs/cards (igual o Termius faz).

### Tab bar (Workspace mode)

Tabs unificadas (áreas + conexões juntas). Áreas fixas no início; `+`
abre menu de "nova conexão / novo terminal local / SFTP". Tabs de
conexão são fechableis com `×` (posição L/R configurável).

Tab ativa puxa cor pro accent dinâmico do chrome (`accent_color` da
conexão; fallback global).

### Áreas vs conexões na tab bar

Distinguir visualmente: áreas têm ícone (vault/folder) e label; conexões
têm `● host_name`. Sem confusão funcional, mesmo coabitando a mesma
linha.

## Caminho crítico

Ordem sugerida. Itens marcados ⊥ podem ir em paralelo.

1. **PR Bugs** (rapid wins, libera valor pro koobs)
   - Right-click paste em SSH
   - Gate AI Chat toggle no setting `ai_enabled`
   - Adicionar `AppTheme::SolarizedDark`
2. **PR Interface section** (rework de settings)
   - Adicionar `SettingsSection::Interface`
   - Absorver Theme cards
   - Adicionar toggles: status bar, close button side, status dot na tab
   - Adicionar `[x] Enable SFTP`
   - Adicionar setting `layout_mode` (UI pode ficar inerte; switching real vem na PR 6)
   - Migration de Theme persisted setting → Interface
3. **PR Host icons + accent system**
   - `Connection.icon_style` + `icon_color` + `accent_color`
   - Migration SQLite
   - Widget `host_icon()` que renderiza Circular/Square/Outline/Initials/Emoji
   - Picker no editor de host (seção Appearance)
4. ⊥ **PR Accent dinâmico no chrome**
   - Header border-bottom com cor da tab ativa
   - Tab indicator
   - Animação cross-fade
5. ⊥ **PR Burger menu**
   - Standalone, funciona no Classic mode também
   - Atalhos Ctrl+1/2/,
6. **PR Workspace mode**
   - `View` 2-níveis, áreas top-level
   - Top tab bar unificada
   - Sidebar contextual (Vault)
   - Terminal full-canvas
   - Setting switching real
   - Default = Workspace pra todos (boot migration)
7. ⊥ **PR Fontes**
   - Bundle JetBrains Mono Nerd Font
   - Enumerar fontes mono via fontdb
   - Auditoria de fallback per-glyph

PRs 1-2 são pré-requisitos pra todas. 3+4 vão juntas (mexem nos mesmos
arquivos). 5 e 7 são paralelas a 6. Cada PR é mergeável independente.

## Riscos abertos

- **Migration de UX**: usuários existentes vão entrar e tudo está
  diferente. Mitigação: changelog detalhado, screenshot animado no
  README, toggle pra Classic em 1 clique em Settings → Interface.
- **Trade dress residual**: mesmo com mitigações, alguém pode reclamar.
  Mitigação: posicionamento explícito como open-source/privacy-first +
  diferenças funcionais reais (P2P sync, MCP, AI chat, plugins de
  cloud) + customização que eles não têm. Risco real continua baixo.
- **Performance da animação accent**: cross-fade em iced 0.13/0.14 não
  é trivial. Fallback: troca instantânea sem animação (mantém o
  benefício de identificação visual, perde só o polish).
- **Nerd Font no binário (+2MB)**: Linux já tem fontconfig que pode
  resolver, mas Windows/macOS fora-da-caixa não. Bundle é a única
  forma de garantir cobertura cross-platform. Setting permite opt-out.
- **fontdb enumeration latency**: scan inicial das fontes do sistema
  pode demorar alguns segundos no Windows. Mitigação: scan em
  background na boot, lista hardcoded como fallback durante scan.

## Sucesso de v0.7

- Usuário fresco instala, vê Workspace mode com Logo Oryxis no canto,
  burger menu, top tabs, sidebar dentro de Vault.
- Abre um host, sidebar some, terminal full-screen, header puxa cor
  laranja do host configurado (accent dinâmico visível).
- Right-click no terminal cola da clipboard direto no SSH.
- Desabilita AI em settings, botão de chat some do terminal.
- Troca o tema pra Solarized Dark, escolhe ícone "Outline" no host,
  vê as bordas circulares com sua cor de accent.
- Box Drawing chars (htop, vim, etc.) renderizam corretamente sem
  precisar instalar fonte extra.
- Vai em Settings → Interface, troca pra Classic mode, sidebar antiga
  volta sem reiniciar.

## Não-objetivos (explícitos)

Itens conversados mas adiados:

- **Sidebar "minimal pure" (esconder de vez no Classic)**: deferido pra
  discussão futura. Workspace mode já resolve o caso de uso.
- **Modos `Tab bar` e `Burger` no Classic**: a discussão de "múltiplas
  formas de navegação" fica restrita a "Classic vs Workspace" em v0.7.
- **System tray + minimize-to-tray**: issue dedicada, v0.8/v0.9.
- **Split panes**: continua adiado pra v0.8.
- **Custom themes do usuário**: v0.9.
