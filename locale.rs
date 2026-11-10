use std::sync::atomic::{AtomicU8, Ordering};
#[cfg(windows)]
use windows::Win32::Globalization::GetUserDefaultUILanguage;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum Language {
    English,
    Russian,
    Chinese,
}

impl Language {
    pub const ALL: [Self; 3] = [Self::English, Self::Russian, Self::Chinese];

    pub fn from_tag(tag: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|lang| lang.tag() == tag)
    }

    pub fn tag(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::Russian => "ru",
            Self::Chinese => "zh-CN",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::Russian => "Русский",
            Self::Chinese => "简体中文",
        }
    }

    #[cfg(windows)]
    fn from_ui_language(id: u16) -> Self {
        match id & 0x03ff {
            0x19 => Self::Russian,
            0x04 => Self::Chinese,
            _ => Self::English,
        }
    }

    #[cfg(windows)]
    pub fn system() -> Self {
        Self::from_ui_language(unsafe { GetUserDefaultUILanguage() })
    }

    /// the Mac: the first of the preferred languages (System Settings), whatever the shell's LANG says;
    /// elsewhere LANGUAGE ("ru:en") / LC_ALL / LC_MESSAGES / LANG ("ru_RU.UTF-8")
    #[cfg(not(windows))]
    pub fn system() -> Self {
        #[cfg(target_os = "macos")]
        let lang = (|| {
            // `(\n    "ru-RU",\n    "en-US"\n)`
            let out = std::process::Command::new("defaults").args(["read", "-g", "AppleLanguages"]).output().ok()?;
            let s = String::from_utf8_lossy(&out.stdout).into_owned();
            s.split(|c: char| c == '(' || c == ',' || c == ')').map(|v| v.trim().trim_matches('"').to_string()).find(|v| !v.is_empty())
        })();
        #[cfg(not(target_os = "macos"))]
        let lang: Option<String> = None;
        let lang = lang.or_else(|| {
            ["LANGUAGE", "LC_ALL", "LC_MESSAGES", "LANG"].into_iter().filter_map(|k| std::env::var(k).ok()).find(|v| !v.is_empty() && v != "C" && v != "POSIX")
        });
        match lang.as_deref().map(|v| v.get(..2).unwrap_or("")) {
            Some("ru") => Self::Russian,
            Some("zh") => Self::Chinese,
            _ => Self::English,
        }
    }
}

static ACTIVE: AtomicU8 = AtomicU8::new(Language::English as u8);

pub fn set(choice: Option<Language>) {
    ACTIVE.store(choice.unwrap_or_else(Language::system) as u8, Ordering::Relaxed);
}

pub fn current() -> Language {
    Language::ALL[ACTIVE.load(Ordering::Relaxed) as usize]
}

pub struct Message(pub &'static str, pub &'static str, pub &'static str);

impl Message {
    pub fn get(&self) -> &'static str {
        match current() {
            Language::English => self.0,
            Language::Russian => self.1,
            Language::Chinese => self.2,
        }
    }
}
