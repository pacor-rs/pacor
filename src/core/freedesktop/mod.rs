use std::fmt::Display;
#[cfg(target_os = "linux")]
use std::path::Path;

use serde::{Deserialize, Serialize};

#[cfg(target_os = "linux")]
use crate::Result;

use self::category::Category;

mod category;

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub(crate) struct FreeDesktop {
    /// The name of the application, default to the name of the executable
    #[serde(skip)]
    name: String,
    #[serde(skip)]
    icon: String,
    #[serde(skip)]
    exec: String,
    #[serde(skip)]
    startup_wm_class: String,
    categories: Vec<Category>,
    comment: Option<String>,
    mime_type: Option<String>,
}

impl Display for FreeDesktop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let categories = self
            .categories
            .iter()
            .map(|c| c.to_string())
            .collect::<Vec<_>>()
            .join(";");
        write!(
            f,
            "[Desktop Entry]\nType=Application\nName={}\nIcon={}\nExec={}\nStartupWMClass={}\nTerminal=false\nCategories={}\nComment={}\nMimeType={}",
            self.name,
            self.icon,
            self.exec,
            self.startup_wm_class,
            categories,
            self.comment.as_deref().unwrap_or_default(),
            self.mime_type.as_deref().unwrap_or_default(),
        )
    }
}

#[cfg(target_os = "linux")]
impl FreeDesktop {
    pub(crate) fn set_name(&mut self, name: &str) {
        self.name = name.to_string();
    }

    pub(crate) fn set_icon<S: Into<String>>(&mut self, icon: S) {
        self.icon = icon.into();
    }

    pub(crate) fn set_exec(&mut self, exec: &str) {
        self.exec = exec.to_string();
        self.startup_wm_class = exec.to_string();
    }

    pub(crate) fn write(&self, path: &Path) -> Result<()> {
        std::fs::write(path, self.to_string())?;

        Ok(())
    }
}
