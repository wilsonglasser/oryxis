use std::collections::HashMap;
use std::sync::Arc;

use crate::CloudProvider;

/// Registered provider entry, keeps the static id alongside the trait
/// object so call sites can iterate over `(id, provider)` without a
/// downcall.
pub struct RegisteredProvider {
    pub id: &'static str,
    pub provider: Arc<dyn CloudProvider>,
}

/// Owns the set of providers the app can use. Concrete provider crates
/// register themselves at app startup; the rest of the codebase pulls
/// providers by id (the same id that lives in `CloudProfile.provider`).
#[derive(Default)]
pub struct CloudProviderRegistry {
    providers: HashMap<&'static str, Arc<dyn CloudProvider>>,
}

impl CloudProviderRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, provider: Arc<dyn CloudProvider>) {
        let id = provider.id();
        self.providers.insert(id, provider);
    }

    pub fn get(&self, id: &str) -> Option<Arc<dyn CloudProvider>> {
        self.providers.get(id).cloned()
    }

    pub fn ids(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.providers.keys().copied()
    }

    pub fn iter(&self) -> impl Iterator<Item = RegisteredProvider> + '_ {
        self.providers
            .iter()
            .map(|(id, p)| RegisteredProvider { id, provider: p.clone() })
    }
}
