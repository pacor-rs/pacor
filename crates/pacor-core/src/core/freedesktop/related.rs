//! Related categories for the freedesktop *additional* category registry.
//!
//! Generated from the registry table in the Desktop Menu Specification:
//! <https://specifications.freedesktop.org/menu/1.1/additional-category-registry.html>
//!
//! The spec defines the relation as:
//!
//! > The Related Categories column lists one or more categories that are
//! > *suggested* to be used in conjunction with the Additional Category. If the
//! > Related Categories column is blank, the Additional Category can be used
//! > with any Main Category.
//!
//! Each entry is an **OR of AND-groups**: the relation holds when every name in
//! at least one group is present. For example `NumericalAnalysis` maps to
//! `[[Education, Math], [Science, Math]]`, meaning "Education **and** Math, or
//! Science **and** Math". Names refer to canonical [`Display`](std::fmt::Display)
//! spellings and may denote main *or* additional categories.

use super::category::AdditionalCategory;

/// Related categories of `category`, as an OR of AND-groups.
///
/// Returns an empty slice for categories that the registry does not constrain.
pub(super) fn related_categories(
    category: &AdditionalCategory,
) -> &'static [&'static [&'static str]] {
    use AdditionalCategory::*;

    match category {
        Building => &[&["Development"]],
        Debugger => &[&["Development"]],
        Ide => &[&["Development"]],
        GUIDesigner => &[&["Development"]],
        Profiling => &[&["Development"]],
        RevisionControl => &[&["Development"]],
        Translation => &[&["Development"]],
        Calendar => &[&["Office"]],
        ContactManagement => &[&["Office"]],
        Database => &[&["Office"], &["Development"], &["AudioVideo"]],
        Dictionary => &[&["Office"], &["TextTools"]],
        Chart => &[&["Office"]],
        Email => &[&["Office"], &["Network"]],
        Finance => &[&["Office"]],
        FlowChart => &[&["Office"]],
        Pda => &[&["Office"]],
        ProjectManagement => &[&["Office"], &["Development"]],
        Presentation => &[&["Office"]],
        Spreadsheet => &[&["Office"]],
        WordProcessor => &[&["Office"]],
        TwoDGraphics => &[&["Graphics"]],
        VectorGraphics => &[&["Graphics", "2DGraphics"]],
        RasterGraphics => &[&["Graphics", "2DGraphics"]],
        ThreeDGraphics => &[&["Graphics"]],
        Scanning => &[&["Graphics"]],
        Ocr => &[&["Graphics", "Scanning"]],
        Photography => &[&["Graphics"], &["Office"]],
        Publishing => &[&["Graphics"], &["Office"]],
        Viewer => &[&["Graphics"], &["Office"]],
        TextTools => &[&["Utility"]],
        DesktopSettings => &[&["Settings"]],
        HardwareSettings => &[&["Settings"]],
        Printing => &[&["HardwareSettings", "Settings"]],
        PackageManager => &[&["Settings"]],
        Dialup => &[&["Network"]],
        InstantMessaging => &[&["Network"]],
        Chat => &[&["Network"]],
        IRCClient => &[&["Network"]],
        Feed => &[&["Network"]],
        FileTransfer => &[&["Network"]],
        HamRadio => &[&["Network"], &["Audio"]],
        News => &[&["Network"]],
        P2P => &[&["Network"]],
        RemoteAccess => &[&["Network"]],
        Telephony => &[&["Network"]],
        TelephonyTools => &[&["Utility"]],
        VideoConference => &[&["Network"]],
        WebBrowser => &[&["Network"]],
        WebDevelopment => &[&["Network"], &["Development"]],
        Midi => &[&["AudioVideo", "Audio"]],
        Mixer => &[&["AudioVideo", "Audio"]],
        Sequencer => &[&["AudioVideo", "Audio"]],
        Tuner => &[&["AudioVideo", "Audio"]],
        TV => &[&["AudioVideo", "Video"]],
        AudioVideoEditing => &[&["Audio"], &["Video"], &["AudioVideo"]],
        Player => &[&["Audio"], &["Video"], &["AudioVideo"]],
        Recorder => &[&["Audio"], &["Video"], &["AudioVideo"]],
        DiscBurning => &[&["AudioVideo"]],
        ActionGame => &[&["Game"]],
        AdventureGame => &[&["Game"]],
        ArcadeGame => &[&["Game"]],
        BoardGame => &[&["Game"]],
        BlocksGame => &[&["Game"]],
        CardGame => &[&["Game"]],
        KidsGame => &[&["Game"]],
        LogicGame => &[&["Game"]],
        RolePlaying => &[&["Game"]],
        Shooter => &[&["Game"]],
        Simulation => &[&["Game"]],
        SportsGame => &[&["Game"]],
        StrategyGame => &[&["Game"]],
        LauncherStore => &[&["Game"]],
        GameTool => &[&["Game"]],
        Art => &[&["Education"], &["Science"]],
        Music => &[&["AudioVideo"], &["Education"]],
        Languages => &[&["Education"], &["Science"]],
        ArtificialIntelligence => &[&["Education"], &["Science"]],
        Astronomy => &[&["Education"], &["Science"]],
        Biology => &[&["Education"], &["Science"]],
        Chemistry => &[&["Education"], &["Science"]],
        ComputerScience => &[&["Education"], &["Science"]],
        DataVisualization => &[&["Education"], &["Science"]],
        Economy => &[&["Education"], &["Science"]],
        Electricity => &[&["Education"], &["Science"]],
        Geography => &[&["Education"], &["Science"]],
        Geology => &[&["Education"], &["Science"]],
        Geoscience => &[&["Education"], &["Science"]],
        History => &[&["Education"], &["Science"]],
        Humanities => &[&["Education"], &["Science"]],
        ImageProcessing => &[&["Education"], &["Science"]],
        Literature => &[&["Education"], &["Science"]],
        Maps => &[&["Education"], &["Science"], &["Utility"]],
        Math => &[&["Education"], &["Science"]],
        NumericalAnalysis => &[&["Education", "Math"], &["Science", "Math"]],
        MedicalSoftware => &[&["Education"], &["Science"]],
        Physics => &[&["Education"], &["Science"]],
        Robotics => &[&["Education"], &["Science"]],
        Spirituality => &[&["Education"], &["Science"], &["Utility"]],
        Sports => &[&["Education"], &["Science"]],
        ParallelComputing => &[
            &["Education", "ComputerScience"],
            &["Science", "ComputerScience"],
        ],
        Archiving => &[&["Utility"]],
        Compression => &[&["Utility", "Archiving"]],
        Emulator => &[&["System"], &["Game"]],
        FileTools => &[&["Utility"], &["System"]],
        FileManager => &[&["System", "FileTools"]],
        TerminalEmulator => &[&["System"]],
        Filesystem => &[&["System"]],
        Monitor => &[&["System"], &["Network"]],
        Security => &[&["Settings"], &["System"]],
        Accessibility => &[&["Settings"], &["Utility"]],
        Calculator => &[&["Utility"]],
        Clock => &[&["Utility"]],
        TextEditor => &[&["Utility"]],
        Kde => &[&["Qt"]],
        Gnome => &[&["GTK"]],
        LXQt => &[&["Qt"]],
        Xfce => &[&["GTK"]],
        Dde => &[&["Qt"]],

        // No registered relation: these may be used with any main category.
        Construction | Amusement | Electronics | Engineering | Documentation | Adult | Core
        | Cosmic | Gtk | Qt | Motif | Java | ConsoleOnly => &[],
    }
}
