use std::fmt::Display;

use crate::config::AppMetadata;

impl Display for AppMetadata {
    /// macOS 的国际化配置信息可查阅：
    ///
    /// <https://developer.apple.com/library/archive/documentation/General/Reference/InfoPlistKeyReference/Articles/AboutInformationPropertyListFiles.html>
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(name) = self.name() {
            writeln!(f, "CFBundleName = \"{}\";", name)?;
        }

        if let Some(display_name) = self.display_name() {
            writeln!(f, "CFBundleDisplayName = \"{}\";", display_name)?;
        }

        if let Some(copyright) = self.copyright() {
            writeln!(f, "NSHumanReadableCopyright = \"{}\";", copyright)?;
        }

        if let Some(description) = self.description() {
            writeln!(f, "CFBundleGetInfoString = \"{}\";", description)?;
        }

        Ok(())
    }
}
