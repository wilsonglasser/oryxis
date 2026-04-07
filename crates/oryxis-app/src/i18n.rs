use std::sync::atomic::{AtomicUsize, Ordering};

static ACTIVE_LANG: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    English,
    PortugueseBR,
    Spanish,
    French,
    German,
    Italian,
    Chinese,
    Japanese,
    Russian,
}

impl Language {
    pub const ALL: &[Language] = &[
        Self::English,
        Self::PortugueseBR,
        Self::Spanish,
        Self::French,
        Self::German,
        Self::Italian,
        Self::Chinese,
        Self::Japanese,
        Self::Russian,
    ];

    pub fn code(&self) -> &'static str {
        match self {
            Self::English => "en",
            Self::PortugueseBR => "pt-BR",
            Self::Spanish => "es",
            Self::French => "fr",
            Self::German => "de",
            Self::Italian => "it",
            Self::Chinese => "zh",
            Self::Japanese => "ja",
            Self::Russian => "ru",
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::English => "English",
            Self::PortugueseBR => "Português (Brasil)",
            Self::Spanish => "Español",
            Self::French => "Français",
            Self::German => "Deutsch",
            Self::Italian => "Italiano",
            Self::Chinese => "中文",
            Self::Japanese => "日本語",
            Self::Russian => "Русский",
        }
    }

    pub fn from_code(code: &str) -> Self {
        match code {
            "pt-BR" => Self::PortugueseBR,
            "es" => Self::Spanish,
            "fr" => Self::French,
            "de" => Self::German,
            "it" => Self::Italian,
            "zh" => Self::Chinese,
            "ja" => Self::Japanese,
            "ru" => Self::Russian,
            _ => Self::English,
        }
    }

    pub fn set_active(lang: Language) {
        let idx = Self::ALL.iter().position(|l| *l == lang).unwrap_or(0);
        ACTIVE_LANG.store(idx, Ordering::Relaxed);
    }

    pub fn active() -> Language {
        let idx = ACTIVE_LANG.load(Ordering::Relaxed);
        Self::ALL.get(idx).copied().unwrap_or(Language::English)
    }
}

impl std::fmt::Display for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

/// Get a translated string. Usage: `t("hosts")` or `t("create_host")`
pub fn t(key: &str) -> &'static str {
    let lang = Language::active();
    translate(key, lang)
}

fn translate(key: &str, lang: Language) -> &'static str {
    match lang {
        Language::English => en(key),
        Language::PortugueseBR => pt_br(key).unwrap_or_else(|| en(key)),
        Language::Spanish => es(key).unwrap_or_else(|| en(key)),
        Language::French => fr(key).unwrap_or_else(|| en(key)),
        Language::German => de(key).unwrap_or_else(|| en(key)),
        Language::Italian => it(key).unwrap_or_else(|| en(key)),
        Language::Chinese => zh(key).unwrap_or_else(|| en(key)),
        Language::Japanese => ja(key).unwrap_or_else(|| en(key)),
        Language::Russian => ru(key).unwrap_or_else(|| en(key)),
    }
}

// =============================================================================
// English (fallback — always returns a value)
// =============================================================================

fn en(key: &str) -> &'static str {
    match key {
        // Navigation
        "hosts" => "Hosts",
        "keychain" => "Keychain",
        "snippets" => "Snippets",
        "known_hosts" => "Known Hosts",
        "history" => "History",
        "settings" => "Settings",
        "local_shell" => "Local Shell",

        // Actions
        "create_host" => "Create host",
        "save" => "Save",
        "cancel" => "Cancel",
        "close" => "Close",
        "delete" => "Delete",
        "edit" => "Edit",
        "connect" => "Connect",
        "duplicate" => "Duplicate",
        "remove" => "Remove",
        "continue_btn" => "Continue",
        "unlock" => "Unlock",
        "import_key" => "Import Key",
        "new_snippet" => "New Snippet",
        "new_identity" => "New Identity",
        "add" => "Add",

        // Host editor
        "edit_host" => "Edit Host",
        "new_host" => "New Host",
        "label" => "Label",
        "hostname" => "Hostname",
        "parent_group" => "Parent Group",
        "ssh_on_port" => "SSH on port",
        "credentials" => "Credentials",
        "username" => "Username",
        "password" => "Password",
        "host_chaining" => "Host Chaining",
        "auth_method" => "Auth Method",
        "disabled" => "Disabled",
        "auto" => "Auto",

        // Empty states
        "create_host_title" => "Create host",
        "create_host_desc" => "Save your connection details as hosts to connect in one click.",
        "add_key_title" => "Add a key",
        "add_key_desc" => "Import SSH keys to authenticate with your hosts.",
        "create_snippet_title" => "Create a snippet",
        "create_snippet_desc" => "Save commands you use often for quick access.",

        // Settings
        "appearance" => "Appearance",
        "theme" => "Theme",
        "terminal_font_size" => "Terminal Font Size",
        "vault_stats" => "Vault Statistics",
        "security" => "Security",
        "lock_vault" => "Lock Vault",
        "about" => "About",
        "terminal_settings" => "Terminal Settings",
        "shortcuts" => "Shortcuts",
        "ai_assistant" => "AI Assistant",
        "language" => "Language",

        // Settings toggles
        "copy_on_select" => "Select text to copy & Right click to paste",
        "bold_bright" => "Use bright colours for bold text",
        "bell_sound" => "Bell sound",
        "keyword_highlight" => "Keyword highlighting",
        "keepalive_interval" => "Keepalive Interval",
        "scrollback" => "Scrollback",
        "vault_password" => "Vault Password",

        // AI Chat
        "ai_chat" => "AI Chat",
        "ask_ai" => "Ask AI...",
        "thinking" => "Thinking...",
        "enable_ai" => "Enable AI Chat",
        "provider" => "Provider",
        "model" => "Model",
        "api_key" => "API Key",
        "api_key_saved" => "API key saved",
        "system_prompt" => "System Prompt",

        // Vault
        "welcome" => "Welcome to Oryxis",
        "master_password" => "Master password",
        "create_vault" => "Create Vault",
        "continue_without_password" => "Continue without password",
        "enter_password" => "Enter your master password to unlock.",
        "forgot_password" => "Forgot password? Reset vault",
        "destroy_vault" => "Yes, destroy vault",
        "vault_destroy_confirm" => "This will permanently delete all saved data.",

        // Terminal shortcuts
        "copy_terminal" => "Copy from Terminal",
        "paste_terminal" => "Paste to Terminal",
        "close_tab" => "Close Tab",
        "switch_tab" => "Switch to Tab 1-9",
        "open_local" => "Open Local Terminal",
        "new_host_shortcut" => "New Host",
        "keyboard_shortcuts" => "Keyboard Shortcuts",

        // Session logs
        "session_logs" => "Session Logs",
        "view_log" => "View Log",
        "duration" => "Duration",
        "in_progress" => "In Progress",

        // Identity
        "identity" => "Identity",
        "managed_by_identity" => "Credentials managed by this identity",
        "no_credentials" => "No credentials",
        "linked_to" => "Linked to",

        // Misc
        "search_hosts" => "Search hosts...",
        "search_keys" => "Search keys...",
        "no_results" => "No results",
        "error" => "Error",
        "version" => "Oryxis v0.1.0",
        "all_hosts" => "All Hosts",
        "set_password" => "Set Password",
        "no_active_connection" => "No active connection",

        _ => "???",
    }
}

// =============================================================================
// Portuguese (Brazil)
// =============================================================================

fn pt_br(key: &str) -> Option<&'static str> {
    Some(match key {
        "hosts" => "Hosts",
        "keychain" => "Chaveiro",
        "snippets" => "Snippets",
        "known_hosts" => "Hosts Conhecidos",
        "history" => "Histórico",
        "settings" => "Configurações",
        "local_shell" => "Shell Local",

        "create_host" => "Criar host",
        "save" => "Salvar",
        "cancel" => "Cancelar",
        "close" => "Fechar",
        "delete" => "Excluir",
        "edit" => "Editar",
        "connect" => "Conectar",
        "duplicate" => "Duplicar",
        "remove" => "Remover",
        "continue_btn" => "Continuar",
        "unlock" => "Desbloquear",
        "import_key" => "Importar Chave",
        "new_snippet" => "Novo Snippet",
        "new_identity" => "Nova Identidade",
        "add" => "Adicionar",

        "edit_host" => "Editar Host",
        "new_host" => "Novo Host",
        "label" => "Rótulo",
        "hostname" => "Endereço",
        "parent_group" => "Grupo",
        "ssh_on_port" => "SSH na porta",
        "credentials" => "Credenciais",
        "username" => "Usuário",
        "password" => "Senha",
        "host_chaining" => "Encadeamento de Host",
        "auth_method" => "Método de Autenticação",
        "disabled" => "Desativado",
        "auto" => "Automático",

        "create_host_title" => "Criar host",
        "create_host_desc" => "Salve os detalhes de conexão como hosts para conectar com um clique.",
        "add_key_title" => "Adicionar chave",
        "add_key_desc" => "Importe chaves SSH para autenticar com seus hosts.",
        "create_snippet_title" => "Criar snippet",
        "create_snippet_desc" => "Salve comandos que você usa frequentemente para acesso rápido.",

        "appearance" => "Aparência",
        "theme" => "Tema",
        "terminal_font_size" => "Tamanho da Fonte do Terminal",
        "vault_stats" => "Estatísticas do Cofre",
        "security" => "Segurança",
        "lock_vault" => "Bloquear Cofre",
        "about" => "Sobre",
        "terminal_settings" => "Configurações do Terminal",
        "shortcuts" => "Atalhos",
        "ai_assistant" => "Assistente IA",
        "language" => "Idioma",

        "copy_on_select" => "Selecionar texto para copiar e clicar com botão direito para colar",
        "bold_bright" => "Usar cores brilhantes para texto em negrito",
        "bell_sound" => "Som do bell",
        "keyword_highlight" => "Destaque de palavras-chave",
        "keepalive_interval" => "Intervalo de Keepalive",
        "scrollback" => "Histórico de rolagem",
        "vault_password" => "Senha do Cofre",

        "ai_chat" => "Chat IA",
        "ask_ai" => "Perguntar à IA...",
        "thinking" => "Pensando...",
        "enable_ai" => "Ativar Chat IA",
        "provider" => "Provedor",
        "model" => "Modelo",
        "api_key" => "Chave da API",
        "api_key_saved" => "Chave da API salva",
        "system_prompt" => "Prompt do Sistema",

        "welcome" => "Bem-vindo ao Oryxis",
        "master_password" => "Senha mestre",
        "create_vault" => "Criar Cofre",
        "continue_without_password" => "Continuar sem senha",
        "enter_password" => "Digite sua senha mestre para desbloquear.",
        "forgot_password" => "Esqueceu a senha? Redefinir cofre",
        "destroy_vault" => "Sim, destruir cofre",
        "vault_destroy_confirm" => "Isso excluirá permanentemente todos os dados salvos.",

        "copy_terminal" => "Copiar do Terminal",
        "paste_terminal" => "Colar no Terminal",
        "close_tab" => "Fechar Aba",
        "switch_tab" => "Alternar para Aba 1-9",
        "open_local" => "Abrir Terminal Local",
        "new_host_shortcut" => "Novo Host",
        "keyboard_shortcuts" => "Atalhos de Teclado",

        "session_logs" => "Logs de Sessão",
        "view_log" => "Ver Log",
        "duration" => "Duração",
        "in_progress" => "Em Andamento",

        "identity" => "Identidade",
        "managed_by_identity" => "Credenciais gerenciadas por esta identidade",
        "no_credentials" => "Sem credenciais",
        "linked_to" => "Vinculado a",

        "search_hosts" => "Buscar hosts...",
        "search_keys" => "Buscar chaves...",
        "no_results" => "Sem resultados",
        "error" => "Erro",
        "version" => "Oryxis v0.1.0",
        "all_hosts" => "Todos os Hosts",
        "set_password" => "Definir Senha",
        "no_active_connection" => "Nenhuma conexão ativa",

        _ => return None,
    })
}

// =============================================================================
// Spanish
// =============================================================================

fn es(key: &str) -> Option<&'static str> {
    Some(match key {
        "hosts" => "Hosts",
        "keychain" => "Llavero",
        "snippets" => "Fragmentos",
        "known_hosts" => "Hosts Conocidos",
        "history" => "Historial",
        "settings" => "Configuración",
        "local_shell" => "Shell Local",

        "create_host" => "Crear host",
        "save" => "Guardar",
        "cancel" => "Cancelar",
        "close" => "Cerrar",
        "delete" => "Eliminar",
        "edit" => "Editar",
        "connect" => "Conectar",
        "duplicate" => "Duplicar",
        "remove" => "Eliminar",
        "continue_btn" => "Continuar",
        "unlock" => "Desbloquear",
        "import_key" => "Importar Clave",
        "new_snippet" => "Nuevo Fragmento",
        "new_identity" => "Nueva Identidad",
        "add" => "Agregar",

        "edit_host" => "Editar Host",
        "new_host" => "Nuevo Host",
        "label" => "Etiqueta",
        "hostname" => "Dirección",
        "parent_group" => "Grupo",
        "ssh_on_port" => "SSH en puerto",
        "credentials" => "Credenciales",
        "username" => "Usuario",
        "password" => "Contraseña",
        "host_chaining" => "Encadenamiento de Host",
        "auth_method" => "Método de Autenticación",
        "disabled" => "Desactivado",
        "auto" => "Automático",

        "create_host_title" => "Crear host",
        "create_host_desc" => "Guarda los detalles de conexión como hosts para conectarte con un clic.",
        "add_key_title" => "Agregar clave",
        "add_key_desc" => "Importa claves SSH para autenticarte con tus hosts.",
        "create_snippet_title" => "Crear fragmento",
        "create_snippet_desc" => "Guarda comandos que usas frecuentemente para acceso rápido.",

        "appearance" => "Apariencia",
        "theme" => "Tema",
        "terminal_font_size" => "Tamaño de Fuente del Terminal",
        "vault_stats" => "Estadísticas del Cofre",
        "security" => "Seguridad",
        "lock_vault" => "Bloquear Cofre",
        "about" => "Acerca de",
        "terminal_settings" => "Configuración del Terminal",
        "shortcuts" => "Atajos",
        "ai_assistant" => "Asistente IA",
        "language" => "Idioma",

        "copy_on_select" => "Seleccionar texto para copiar y clic derecho para pegar",
        "bold_bright" => "Usar colores brillantes para texto en negrita",
        "bell_sound" => "Sonido de campana",
        "keyword_highlight" => "Resaltado de palabras clave",
        "keepalive_interval" => "Intervalo de Keepalive",
        "scrollback" => "Historial de desplazamiento",
        "vault_password" => "Contraseña del Cofre",

        "ai_chat" => "Chat IA",
        "ask_ai" => "Preguntar a la IA...",
        "thinking" => "Pensando...",
        "enable_ai" => "Activar Chat IA",
        "provider" => "Proveedor",
        "model" => "Modelo",
        "api_key" => "Clave API",
        "api_key_saved" => "Clave API guardada",
        "system_prompt" => "Prompt del Sistema",

        "welcome" => "Bienvenido a Oryxis",
        "master_password" => "Contraseña maestra",
        "create_vault" => "Crear Cofre",
        "continue_without_password" => "Continuar sin contraseña",
        "enter_password" => "Ingresa tu contraseña maestra para desbloquear.",
        "forgot_password" => "¿Olvidaste la contraseña? Restablecer cofre",
        "destroy_vault" => "Sí, destruir cofre",
        "vault_destroy_confirm" => "Esto eliminará permanentemente todos los datos guardados.",

        "copy_terminal" => "Copiar del Terminal",
        "paste_terminal" => "Pegar en Terminal",
        "close_tab" => "Cerrar Pestaña",
        "switch_tab" => "Cambiar a Pestaña 1-9",
        "open_local" => "Abrir Terminal Local",
        "new_host_shortcut" => "Nuevo Host",
        "keyboard_shortcuts" => "Atajos de Teclado",

        "session_logs" => "Registros de Sesión",
        "view_log" => "Ver Registro",
        "duration" => "Duración",
        "in_progress" => "En Progreso",

        "identity" => "Identidad",
        "managed_by_identity" => "Credenciales gestionadas por esta identidad",
        "no_credentials" => "Sin credenciales",
        "linked_to" => "Vinculado a",

        "search_hosts" => "Buscar hosts...",
        "search_keys" => "Buscar claves...",
        "no_results" => "Sin resultados",
        "error" => "Error",
        "version" => "Oryxis v0.1.0",
        "all_hosts" => "Todos los Hosts",
        "set_password" => "Establecer Contraseña",
        "no_active_connection" => "Sin conexión activa",

        _ => return None,
    })
}

// =============================================================================
// French
// =============================================================================

fn fr(key: &str) -> Option<&'static str> {
    Some(match key {
        "hosts" => "Hôtes",
        "keychain" => "Trousseau",
        "snippets" => "Extraits",
        "known_hosts" => "Hôtes Connus",
        "history" => "Historique",
        "settings" => "Paramètres",
        "local_shell" => "Shell Local",

        "create_host" => "Créer un hôte",
        "save" => "Enregistrer",
        "cancel" => "Annuler",
        "close" => "Fermer",
        "delete" => "Supprimer",
        "edit" => "Modifier",
        "connect" => "Connexion",
        "duplicate" => "Dupliquer",
        "remove" => "Retirer",
        "continue_btn" => "Continuer",
        "unlock" => "Déverrouiller",
        "import_key" => "Importer Clé",
        "new_snippet" => "Nouvel Extrait",
        "new_identity" => "Nouvelle Identité",
        "add" => "Ajouter",

        "edit_host" => "Modifier l'Hôte",
        "new_host" => "Nouvel Hôte",
        "label" => "Libellé",
        "hostname" => "Adresse",
        "parent_group" => "Groupe",
        "ssh_on_port" => "SSH sur le port",
        "credentials" => "Identifiants",
        "username" => "Utilisateur",
        "password" => "Mot de passe",
        "host_chaining" => "Chaînage d'Hôtes",
        "auth_method" => "Méthode d'Authentification",
        "disabled" => "Désactivé",
        "auto" => "Automatique",

        "create_host_title" => "Créer un hôte",
        "create_host_desc" => "Enregistrez vos détails de connexion pour vous connecter en un clic.",
        "add_key_title" => "Ajouter une clé",
        "add_key_desc" => "Importez des clés SSH pour vous authentifier auprès de vos hôtes.",
        "create_snippet_title" => "Créer un extrait",
        "create_snippet_desc" => "Enregistrez les commandes que vous utilisez souvent pour un accès rapide.",

        "appearance" => "Apparence",
        "theme" => "Thème",
        "terminal_font_size" => "Taille de Police du Terminal",
        "vault_stats" => "Statistiques du Coffre",
        "security" => "Sécurité",
        "lock_vault" => "Verrouiller le Coffre",
        "about" => "À propos",
        "terminal_settings" => "Paramètres du Terminal",
        "shortcuts" => "Raccourcis",
        "ai_assistant" => "Assistant IA",
        "language" => "Langue",

        "copy_on_select" => "Sélectionner pour copier et clic droit pour coller",
        "bold_bright" => "Utiliser des couleurs vives pour le texte en gras",
        "bell_sound" => "Son de la cloche",
        "keyword_highlight" => "Surlignage des mots-clés",
        "keepalive_interval" => "Intervalle de Keepalive",
        "scrollback" => "Historique de défilement",
        "vault_password" => "Mot de passe du Coffre",

        "ai_chat" => "Chat IA",
        "ask_ai" => "Demander à l'IA...",
        "thinking" => "Réflexion...",
        "enable_ai" => "Activer le Chat IA",
        "provider" => "Fournisseur",
        "model" => "Modèle",
        "api_key" => "Clé API",
        "api_key_saved" => "Clé API enregistrée",
        "system_prompt" => "Prompt Système",

        "welcome" => "Bienvenue sur Oryxis",
        "master_password" => "Mot de passe maître",
        "create_vault" => "Créer le Coffre",
        "continue_without_password" => "Continuer sans mot de passe",
        "enter_password" => "Entrez votre mot de passe maître pour déverrouiller.",
        "forgot_password" => "Mot de passe oublié ? Réinitialiser le coffre",
        "destroy_vault" => "Oui, détruire le coffre",
        "vault_destroy_confirm" => "Cela supprimera définitivement toutes les données enregistrées.",

        "copy_terminal" => "Copier depuis le Terminal",
        "paste_terminal" => "Coller dans le Terminal",
        "close_tab" => "Fermer l'Onglet",
        "switch_tab" => "Basculer vers l'Onglet 1-9",
        "open_local" => "Ouvrir un Terminal Local",
        "new_host_shortcut" => "Nouvel Hôte",
        "keyboard_shortcuts" => "Raccourcis Clavier",

        "session_logs" => "Journaux de Session",
        "view_log" => "Voir le Journal",
        "duration" => "Durée",
        "in_progress" => "En Cours",

        "identity" => "Identité",
        "managed_by_identity" => "Identifiants gérés par cette identité",
        "no_credentials" => "Aucun identifiant",
        "linked_to" => "Lié à",

        "search_hosts" => "Rechercher des hôtes...",
        "search_keys" => "Rechercher des clés...",
        "no_results" => "Aucun résultat",
        "error" => "Erreur",
        "version" => "Oryxis v0.1.0",
        "all_hosts" => "Tous les Hôtes",
        "set_password" => "Définir le Mot de passe",
        "no_active_connection" => "Aucune connexion active",

        _ => return None,
    })
}

// =============================================================================
// German
// =============================================================================

fn de(key: &str) -> Option<&'static str> {
    Some(match key {
        "hosts" => "Hosts",
        "keychain" => "Schlüsselbund",
        "snippets" => "Snippets",
        "known_hosts" => "Bekannte Hosts",
        "history" => "Verlauf",
        "settings" => "Einstellungen",
        "local_shell" => "Lokale Shell",

        "create_host" => "Host erstellen",
        "save" => "Speichern",
        "cancel" => "Abbrechen",
        "close" => "Schließen",
        "delete" => "Löschen",
        "edit" => "Bearbeiten",
        "connect" => "Verbinden",
        "duplicate" => "Duplizieren",
        "remove" => "Entfernen",
        "continue_btn" => "Weiter",
        "unlock" => "Entsperren",
        "import_key" => "Schlüssel Importieren",
        "new_snippet" => "Neues Snippet",
        "new_identity" => "Neue Identität",
        "add" => "Hinzufügen",

        "edit_host" => "Host Bearbeiten",
        "new_host" => "Neuer Host",
        "label" => "Bezeichnung",
        "hostname" => "Hostname",
        "parent_group" => "Gruppe",
        "ssh_on_port" => "SSH auf Port",
        "credentials" => "Zugangsdaten",
        "username" => "Benutzername",
        "password" => "Passwort",
        "host_chaining" => "Host-Verkettung",
        "auth_method" => "Authentifizierungsmethode",
        "disabled" => "Deaktiviert",
        "auto" => "Automatisch",

        "create_host_title" => "Host erstellen",
        "create_host_desc" => "Speichern Sie Ihre Verbindungsdaten als Hosts, um sich mit einem Klick zu verbinden.",
        "add_key_title" => "Schlüssel hinzufügen",
        "add_key_desc" => "Importieren Sie SSH-Schlüssel zur Authentifizierung bei Ihren Hosts.",
        "create_snippet_title" => "Snippet erstellen",
        "create_snippet_desc" => "Speichern Sie häufig verwendete Befehle für schnellen Zugriff.",

        "appearance" => "Darstellung",
        "theme" => "Design",
        "terminal_font_size" => "Terminal-Schriftgröße",
        "vault_stats" => "Tresor-Statistiken",
        "security" => "Sicherheit",
        "lock_vault" => "Tresor Sperren",
        "about" => "Über",
        "terminal_settings" => "Terminal-Einstellungen",
        "shortcuts" => "Tastenkürzel",
        "ai_assistant" => "KI-Assistent",
        "language" => "Sprache",

        "copy_on_select" => "Text auswählen zum Kopieren & Rechtsklick zum Einfügen",
        "bold_bright" => "Helle Farben für fetten Text verwenden",
        "bell_sound" => "Glockenton",
        "keyword_highlight" => "Schlüsselwort-Hervorhebung",
        "keepalive_interval" => "Keepalive-Intervall",
        "scrollback" => "Scrollverlauf",
        "vault_password" => "Tresor-Passwort",

        "ai_chat" => "KI-Chat",
        "ask_ai" => "KI fragen...",
        "thinking" => "Denke nach...",
        "enable_ai" => "KI-Chat Aktivieren",
        "provider" => "Anbieter",
        "model" => "Modell",
        "api_key" => "API-Schlüssel",
        "api_key_saved" => "API-Schlüssel gespeichert",
        "system_prompt" => "System-Prompt",

        "welcome" => "Willkommen bei Oryxis",
        "master_password" => "Master-Passwort",
        "create_vault" => "Tresor Erstellen",
        "continue_without_password" => "Ohne Passwort fortfahren",
        "enter_password" => "Geben Sie Ihr Master-Passwort zum Entsperren ein.",
        "forgot_password" => "Passwort vergessen? Tresor zurücksetzen",
        "destroy_vault" => "Ja, Tresor zerstören",
        "vault_destroy_confirm" => "Dies wird alle gespeicherten Daten dauerhaft löschen.",

        "copy_terminal" => "Aus Terminal kopieren",
        "paste_terminal" => "In Terminal einfügen",
        "close_tab" => "Tab Schließen",
        "switch_tab" => "Zu Tab 1-9 wechseln",
        "open_local" => "Lokales Terminal öffnen",
        "new_host_shortcut" => "Neuer Host",
        "keyboard_shortcuts" => "Tastenkürzel",

        "session_logs" => "Sitzungsprotokolle",
        "view_log" => "Protokoll anzeigen",
        "duration" => "Dauer",
        "in_progress" => "In Bearbeitung",

        "identity" => "Identität",
        "managed_by_identity" => "Zugangsdaten werden von dieser Identität verwaltet",
        "no_credentials" => "Keine Zugangsdaten",
        "linked_to" => "Verknüpft mit",

        "search_hosts" => "Hosts suchen...",
        "search_keys" => "Schlüssel suchen...",
        "no_results" => "Keine Ergebnisse",
        "error" => "Fehler",
        "version" => "Oryxis v0.1.0",
        "all_hosts" => "Alle Hosts",
        "set_password" => "Passwort Festlegen",
        "no_active_connection" => "Keine aktive Verbindung",

        _ => return None,
    })
}

// =============================================================================
// Italian
// =============================================================================

fn it(key: &str) -> Option<&'static str> {
    Some(match key {
        "hosts" => "Host",
        "keychain" => "Portachiavi",
        "snippets" => "Frammenti",
        "known_hosts" => "Host Conosciuti",
        "history" => "Cronologia",
        "settings" => "Impostazioni",
        "local_shell" => "Shell Locale",

        "create_host" => "Crea host",
        "save" => "Salva",
        "cancel" => "Annulla",
        "close" => "Chiudi",
        "delete" => "Elimina",
        "edit" => "Modifica",
        "connect" => "Connetti",
        "duplicate" => "Duplica",
        "remove" => "Rimuovi",
        "continue_btn" => "Continua",
        "unlock" => "Sblocca",
        "import_key" => "Importa Chiave",
        "new_snippet" => "Nuovo Frammento",
        "new_identity" => "Nuova Identità",
        "add" => "Aggiungi",

        "edit_host" => "Modifica Host",
        "new_host" => "Nuovo Host",
        "label" => "Etichetta",
        "hostname" => "Indirizzo",
        "parent_group" => "Gruppo",
        "ssh_on_port" => "SSH sulla porta",
        "credentials" => "Credenziali",
        "username" => "Nome utente",
        "password" => "Password",
        "host_chaining" => "Concatenamento Host",
        "auth_method" => "Metodo di Autenticazione",
        "disabled" => "Disattivato",
        "auto" => "Automatico",

        "create_host_title" => "Crea host",
        "create_host_desc" => "Salva i dettagli di connessione come host per connetterti con un clic.",
        "add_key_title" => "Aggiungi chiave",
        "add_key_desc" => "Importa chiavi SSH per autenticarti con i tuoi host.",
        "create_snippet_title" => "Crea frammento",
        "create_snippet_desc" => "Salva i comandi che usi spesso per un accesso rapido.",

        "appearance" => "Aspetto",
        "theme" => "Tema",
        "terminal_font_size" => "Dimensione Font del Terminale",
        "vault_stats" => "Statistiche del Vault",
        "security" => "Sicurezza",
        "lock_vault" => "Blocca Vault",
        "about" => "Info",
        "terminal_settings" => "Impostazioni Terminale",
        "shortcuts" => "Scorciatoie",
        "ai_assistant" => "Assistente IA",
        "language" => "Lingua",

        "copy_on_select" => "Seleziona il testo per copiare e clic destro per incollare",
        "bold_bright" => "Usa colori vivaci per il testo in grassetto",
        "bell_sound" => "Suono campanella",
        "keyword_highlight" => "Evidenziazione parole chiave",
        "keepalive_interval" => "Intervallo Keepalive",
        "scrollback" => "Cronologia scorrimento",
        "vault_password" => "Password del Vault",

        "ai_chat" => "Chat IA",
        "ask_ai" => "Chiedi all'IA...",
        "thinking" => "Sto pensando...",
        "enable_ai" => "Attiva Chat IA",
        "provider" => "Fornitore",
        "model" => "Modello",
        "api_key" => "Chiave API",
        "api_key_saved" => "Chiave API salvata",
        "system_prompt" => "Prompt di Sistema",

        "welcome" => "Benvenuto su Oryxis",
        "master_password" => "Password principale",
        "create_vault" => "Crea Vault",
        "continue_without_password" => "Continua senza password",
        "enter_password" => "Inserisci la tua password principale per sbloccare.",
        "forgot_password" => "Password dimenticata? Reimposta il vault",
        "destroy_vault" => "Sì, distruggi il vault",
        "vault_destroy_confirm" => "Questo eliminerà permanentemente tutti i dati salvati.",

        "copy_terminal" => "Copia dal Terminale",
        "paste_terminal" => "Incolla nel Terminale",
        "close_tab" => "Chiudi Scheda",
        "switch_tab" => "Passa alla Scheda 1-9",
        "open_local" => "Apri Terminale Locale",
        "new_host_shortcut" => "Nuovo Host",
        "keyboard_shortcuts" => "Scorciatoie da Tastiera",

        "session_logs" => "Registri di Sessione",
        "view_log" => "Visualizza Registro",
        "duration" => "Durata",
        "in_progress" => "In Corso",

        "identity" => "Identità",
        "managed_by_identity" => "Credenziali gestite da questa identità",
        "no_credentials" => "Nessuna credenziale",
        "linked_to" => "Collegato a",

        "search_hosts" => "Cerca host...",
        "search_keys" => "Cerca chiavi...",
        "no_results" => "Nessun risultato",
        "error" => "Errore",
        "version" => "Oryxis v0.1.0",
        "all_hosts" => "Tutti gli Host",
        "set_password" => "Imposta Password",
        "no_active_connection" => "Nessuna connessione attiva",

        _ => return None,
    })
}

// =============================================================================
// Chinese (Simplified)
// =============================================================================

fn zh(key: &str) -> Option<&'static str> {
    Some(match key {
        "hosts" => "主机",
        "keychain" => "密钥链",
        "snippets" => "代码片段",
        "known_hosts" => "已知主机",
        "history" => "历史记录",
        "settings" => "设置",
        "local_shell" => "本地终端",

        "create_host" => "创建主机",
        "save" => "保存",
        "cancel" => "取消",
        "close" => "关闭",
        "delete" => "删除",
        "edit" => "编辑",
        "connect" => "连接",
        "duplicate" => "复制",
        "remove" => "移除",
        "continue_btn" => "继续",
        "unlock" => "解锁",
        "import_key" => "导入密钥",
        "new_snippet" => "新建片段",
        "new_identity" => "新建身份",
        "add" => "添加",

        "edit_host" => "编辑主机",
        "new_host" => "新建主机",
        "label" => "标签",
        "hostname" => "主机名",
        "parent_group" => "上级分组",
        "ssh_on_port" => "SSH 端口",
        "credentials" => "凭据",
        "username" => "用户名",
        "password" => "密码",
        "host_chaining" => "主机链接",
        "auth_method" => "认证方式",
        "disabled" => "已禁用",
        "auto" => "自动",

        "create_host_title" => "创建主机",
        "create_host_desc" => "保存连接信息，一键连接您的主机。",
        "add_key_title" => "添加密钥",
        "add_key_desc" => "导入 SSH 密钥以验证您的主机身份。",
        "create_snippet_title" => "创建代码片段",
        "create_snippet_desc" => "保存常用命令以便快速访问。",

        "appearance" => "外观",
        "theme" => "主题",
        "terminal_font_size" => "终端字体大小",
        "vault_stats" => "保险库统计",
        "security" => "安全",
        "lock_vault" => "锁定保险库",
        "about" => "关于",
        "terminal_settings" => "终端设置",
        "shortcuts" => "快捷键",
        "ai_assistant" => "AI 助手",
        "language" => "语言",

        "copy_on_select" => "选中文本自动复制，右键粘贴",
        "bold_bright" => "粗体文本使用高亮颜色",
        "bell_sound" => "响铃声音",
        "keyword_highlight" => "关键词高亮",
        "keepalive_interval" => "心跳间隔",
        "scrollback" => "回滚行数",
        "vault_password" => "保险库密码",

        "ai_chat" => "AI 对话",
        "ask_ai" => "向 AI 提问...",
        "thinking" => "思考中...",
        "enable_ai" => "启用 AI 对话",
        "provider" => "服务商",
        "model" => "模型",
        "api_key" => "API 密钥",
        "api_key_saved" => "API 密钥已保存",
        "system_prompt" => "系统提示词",

        "welcome" => "欢迎使用 Oryxis",
        "master_password" => "主密码",
        "create_vault" => "创建保险库",
        "continue_without_password" => "不设密码继续",
        "enter_password" => "输入主密码以解锁。",
        "forgot_password" => "忘记密码？重置保险库",
        "destroy_vault" => "确认销毁保险库",
        "vault_destroy_confirm" => "这将永久删除所有已保存的数据。",

        "copy_terminal" => "从终端复制",
        "paste_terminal" => "粘贴到终端",
        "close_tab" => "关闭标签页",
        "switch_tab" => "切换到标签页 1-9",
        "open_local" => "打开本地终端",
        "new_host_shortcut" => "新建主机",
        "keyboard_shortcuts" => "键盘快捷键",

        "session_logs" => "会话日志",
        "view_log" => "查看日志",
        "duration" => "持续时间",
        "in_progress" => "进行中",

        "identity" => "身份",
        "managed_by_identity" => "凭据由此身份管理",
        "no_credentials" => "无凭据",
        "linked_to" => "关联到",

        "search_hosts" => "搜索主机...",
        "search_keys" => "搜索密钥...",
        "no_results" => "无结果",
        "error" => "错误",
        "version" => "Oryxis v0.1.0",
        "all_hosts" => "所有主机",
        "set_password" => "设置密码",
        "no_active_connection" => "无活动连接",

        _ => return None,
    })
}

// =============================================================================
// Japanese
// =============================================================================

fn ja(key: &str) -> Option<&'static str> {
    Some(match key {
        "hosts" => "ホスト",
        "keychain" => "キーチェーン",
        "snippets" => "スニペット",
        "known_hosts" => "既知のホスト",
        "history" => "履歴",
        "settings" => "設定",
        "local_shell" => "ローカルシェル",

        "create_host" => "ホストを作成",
        "save" => "保存",
        "cancel" => "キャンセル",
        "close" => "閉じる",
        "delete" => "削除",
        "edit" => "編集",
        "connect" => "接続",
        "duplicate" => "複製",
        "remove" => "削除",
        "continue_btn" => "続行",
        "unlock" => "ロック解除",
        "import_key" => "鍵をインポート",
        "new_snippet" => "新しいスニペット",
        "new_identity" => "新しいID",
        "add" => "追加",

        "edit_host" => "ホストを編集",
        "new_host" => "新しいホスト",
        "label" => "ラベル",
        "hostname" => "ホスト名",
        "parent_group" => "親グループ",
        "ssh_on_port" => "SSHポート",
        "credentials" => "認証情報",
        "username" => "ユーザー名",
        "password" => "パスワード",
        "host_chaining" => "ホストチェーン",
        "auth_method" => "認証方式",
        "disabled" => "無効",
        "auto" => "自動",

        "create_host_title" => "ホストを作成",
        "create_host_desc" => "接続情報をホストとして保存し、ワンクリックで接続できます。",
        "add_key_title" => "鍵を追加",
        "add_key_desc" => "SSH鍵をインポートしてホストへの認証に使用します。",
        "create_snippet_title" => "スニペットを作成",
        "create_snippet_desc" => "よく使うコマンドを保存して素早くアクセスできます。",

        "appearance" => "外観",
        "theme" => "テーマ",
        "terminal_font_size" => "ターミナルフォントサイズ",
        "vault_stats" => "ボールト統計",
        "security" => "セキュリティ",
        "lock_vault" => "ボールトをロック",
        "about" => "バージョン情報",
        "terminal_settings" => "ターミナル設定",
        "shortcuts" => "ショートカット",
        "ai_assistant" => "AIアシスタント",
        "language" => "言語",

        "copy_on_select" => "テキスト選択でコピー、右クリックでペースト",
        "bold_bright" => "太字テキストに明るい色を使用",
        "bell_sound" => "ベル音",
        "keyword_highlight" => "キーワードハイライト",
        "keepalive_interval" => "キープアライブ間隔",
        "scrollback" => "スクロールバック",
        "vault_password" => "ボールトパスワード",

        "ai_chat" => "AIチャット",
        "ask_ai" => "AIに質問...",
        "thinking" => "考え中...",
        "enable_ai" => "AIチャットを有効化",
        "provider" => "プロバイダー",
        "model" => "モデル",
        "api_key" => "APIキー",
        "api_key_saved" => "APIキーを保存しました",
        "system_prompt" => "システムプロンプト",

        "welcome" => "Oryxisへようこそ",
        "master_password" => "マスターパスワード",
        "create_vault" => "ボールトを作成",
        "continue_without_password" => "パスワードなしで続行",
        "enter_password" => "マスターパスワードを入力してロックを解除してください。",
        "forgot_password" => "パスワードを忘れた場合はボールトをリセット",
        "destroy_vault" => "はい、ボールトを破棄します",
        "vault_destroy_confirm" => "保存されたすべてのデータが完全に削除されます。",

        "copy_terminal" => "ターミナルからコピー",
        "paste_terminal" => "ターミナルにペースト",
        "close_tab" => "タブを閉じる",
        "switch_tab" => "タブ1-9に切り替え",
        "open_local" => "ローカルターミナルを開く",
        "new_host_shortcut" => "新しいホスト",
        "keyboard_shortcuts" => "キーボードショートカット",

        "session_logs" => "セッションログ",
        "view_log" => "ログを表示",
        "duration" => "所要時間",
        "in_progress" => "進行中",

        "identity" => "ID",
        "managed_by_identity" => "このIDにより認証情報が管理されています",
        "no_credentials" => "認証情報なし",
        "linked_to" => "リンク先",

        "search_hosts" => "ホストを検索...",
        "search_keys" => "鍵を検索...",
        "no_results" => "結果なし",
        "error" => "エラー",
        "version" => "Oryxis v0.1.0",
        "all_hosts" => "すべてのホスト",
        "set_password" => "パスワードを設定",
        "no_active_connection" => "アクティブな接続なし",

        _ => return None,
    })
}

// =============================================================================
// Russian
// =============================================================================

fn ru(key: &str) -> Option<&'static str> {
    Some(match key {
        "hosts" => "Хосты",
        "keychain" => "Связка ключей",
        "snippets" => "Сниппеты",
        "known_hosts" => "Известные хосты",
        "history" => "История",
        "settings" => "Настройки",
        "local_shell" => "Локальная оболочка",

        "create_host" => "Создать хост",
        "save" => "Сохранить",
        "cancel" => "Отмена",
        "close" => "Закрыть",
        "delete" => "Удалить",
        "edit" => "Редактировать",
        "connect" => "Подключить",
        "duplicate" => "Дублировать",
        "remove" => "Удалить",
        "continue_btn" => "Продолжить",
        "unlock" => "Разблокировать",
        "import_key" => "Импорт ключа",
        "new_snippet" => "Новый сниппет",
        "new_identity" => "Новая личность",
        "add" => "Добавить",

        "edit_host" => "Редактировать хост",
        "new_host" => "Новый хост",
        "label" => "Метка",
        "hostname" => "Имя хоста",
        "parent_group" => "Группа",
        "ssh_on_port" => "SSH на порту",
        "credentials" => "Учётные данные",
        "username" => "Имя пользователя",
        "password" => "Пароль",
        "host_chaining" => "Цепочка хостов",
        "auth_method" => "Метод аутентификации",
        "disabled" => "Отключено",
        "auto" => "Авто",

        "create_host_title" => "Создать хост",
        "create_host_desc" => "Сохраните данные подключения для быстрого доступа одним кликом.",
        "add_key_title" => "Добавить ключ",
        "add_key_desc" => "Импортируйте SSH-ключи для аутентификации на ваших хостах.",
        "create_snippet_title" => "Создать сниппет",
        "create_snippet_desc" => "Сохраните часто используемые команды для быстрого доступа.",

        "appearance" => "Внешний вид",
        "theme" => "Тема",
        "terminal_font_size" => "Размер шрифта терминала",
        "vault_stats" => "Статистика хранилища",
        "security" => "Безопасность",
        "lock_vault" => "Заблокировать хранилище",
        "about" => "О программе",
        "terminal_settings" => "Настройки терминала",
        "shortcuts" => "Горячие клавиши",
        "ai_assistant" => "ИИ-ассистент",
        "language" => "Язык",

        "copy_on_select" => "Копировать при выделении, вставка правой кнопкой",
        "bold_bright" => "Яркие цвета для жирного текста",
        "bell_sound" => "Звук звонка",
        "keyword_highlight" => "Подсветка ключевых слов",
        "keepalive_interval" => "Интервал Keepalive",
        "scrollback" => "Буфер прокрутки",
        "vault_password" => "Пароль хранилища",

        "ai_chat" => "ИИ-чат",
        "ask_ai" => "Спросить ИИ...",
        "thinking" => "Думаю...",
        "enable_ai" => "Включить ИИ-чат",
        "provider" => "Провайдер",
        "model" => "Модель",
        "api_key" => "API-ключ",
        "api_key_saved" => "API-ключ сохранён",
        "system_prompt" => "Системный промпт",

        "welcome" => "Добро пожаловать в Oryxis",
        "master_password" => "Мастер-пароль",
        "create_vault" => "Создать хранилище",
        "continue_without_password" => "Продолжить без пароля",
        "enter_password" => "Введите мастер-пароль для разблокировки.",
        "forgot_password" => "Забыли пароль? Сбросить хранилище",
        "destroy_vault" => "Да, уничтожить хранилище",
        "vault_destroy_confirm" => "Все сохранённые данные будут безвозвратно удалены.",

        "copy_terminal" => "Копировать из терминала",
        "paste_terminal" => "Вставить в терминал",
        "close_tab" => "Закрыть вкладку",
        "switch_tab" => "Переключить на вкладку 1-9",
        "open_local" => "Открыть локальный терминал",
        "new_host_shortcut" => "Новый хост",
        "keyboard_shortcuts" => "Горячие клавиши",

        "session_logs" => "Журналы сеансов",
        "view_log" => "Просмотреть журнал",
        "duration" => "Длительность",
        "in_progress" => "В процессе",

        "identity" => "Личность",
        "managed_by_identity" => "Учётные данные управляются этой личностью",
        "no_credentials" => "Нет учётных данных",
        "linked_to" => "Связан с",

        "search_hosts" => "Поиск хостов...",
        "search_keys" => "Поиск ключей...",
        "no_results" => "Нет результатов",
        "error" => "Ошибка",
        "version" => "Oryxis v0.1.0",
        "all_hosts" => "Все хосты",
        "set_password" => "Установить пароль",
        "no_active_connection" => "Нет активного подключения",

        _ => return None,
    })
}
