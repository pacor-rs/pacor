use std::fmt::{Debug, Display};

use serde::{Deserialize, Serialize};

/// https://specifications.freedesktop.org/menu-spec/latest/category-registry.html#main-category-registry
#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) enum MainCategory {
    /// Application for presenting, creating, or processing multimedia (audio/video)
    AudioVideo,

    /// An audio application
    ///
    /// **Note**: Desktop entry must include AudioVideo as well
    Audio,

    /// A video application
    ///
    /// **Note**: Desktop entry must include AudioVideo as well
    Video,

    /// An application for development  
    Development,

    /// Educational software
    Education,

    /// A game
    Game,

    /// Application for viewing, creating, or processing graphics
    Graphics,

    /// Network application such as a web browser
    Network,

    /// An office type application
    Office,

    /// Scientific software
    ///
    /// **Note**: Entries may appear in a separate menu or as part of a "Control Center"
    Science,

    /// Settings applications
    Settings,

    /// System application, "System Tools" such as say a log viewer or network monitor
    System,

    /// Small utility application, "Accessories"
    Utility,
}

impl Display for MainCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

/// https://specifications.freedesktop.org/menu-spec/latest/additional-category-registry.html
#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) enum AdditionalCategory {
    /// A tool to build applications
    ///
    /// **Related Categories**: Development
    Building,

    /// A tool to debug applications
    ///
    /// **Related Categories**: Development
    Debugger,

    /// IDE application
    ///
    /// **Related Categories**: Development
    Ide,

    /// A GUI designer application
    ///
    /// **Related Categories**: Development
    GUIDesigner,

    /// A profiling tool
    ///
    /// **Related Categories**: Development
    Profiling,

    /// Applications like cvs or subversion
    ///
    /// **Related Categories**: Development
    RevisionControl,

    /// A translation tool
    ///
    /// **Related Categories**: Development
    Translation,

    /// Calendar application
    ///
    /// **Related Categories**: Office
    Calendar,

    /// E.g. an address book
    ///
    /// **Related Categories**: Office
    ContactManagement,

    /// Application to manage a database
    ///
    /// **Related Categories**: Office or Development or AudioVideo
    Database,

    /// A dictionary
    ///
    /// **Related Categories**: Office or TextTools
    Dictionary,

    /// Chart application
    ///
    /// **Related Categories**: Office
    Chart,

    /// Email application
    ///
    /// **Related Categories**: Office or Network
    Email,

    /// Application to manage your finance
    ///
    /// **Related Categories**: Office
    Finance,

    /// A flowchart application
    ///
    /// **Related Categories**: Office
    FlowChart,

    /// Tool to manage your PDA
    ///
    /// **Related Categories**: Office
    Pda,

    /// Project management application
    ///
    /// **Related Categories**: Office or Development
    ProjectManagement,

    /// Presentation software
    ///
    /// **Related Categories**: Office
    Presentation,

    /// A spreadsheet
    ///
    /// **Related Categories**: Office
    Spreadsheet,

    /// A word processor
    ///
    /// **Related Categories**: Office
    WordProcessor,

    /// 2D based graphical application
    ///
    /// **Related Categories**: Graphics
    TwoDGraphics,

    /// Application for viewing, creating, or processing vector graphics
    ///
    /// **Related Categories**: Graphics;2DGraphics
    VectorGraphics,

    /// Application for viewing, creating, or processing raster (bitmap) graphics
    ///
    /// **Related Categories**: Graphics;2DGraphics
    RasterGraphics,

    /// Application for viewing, creating, or processing 3-D graphics
    ///
    /// **Related Categories**: Graphics
    ThreeDGraphics,

    /// Tool to scan a file/text
    ///
    /// **Related Categories**: Graphics
    Scanning,

    /// Optical character recognition application
    ///
    /// **Related Categories**: Graphics;Scanning
    Ocr,

    /// Camera tools, etc.
    ///
    /// **Related Categories**: Graphics or Office
    Photography,

    /// Desktop Publishing applications and Color Management tools
    ///
    /// **Related Categories**: Graphics or Office
    Publishing,

    /// Tool to view e.g. a graphic or pdf file
    ///
    /// **Related Categories**: Graphics or Office
    Viewer,

    /// A text tool utility
    ///
    /// **Related Categories**: Utility
    TextTools,

    /// Configuration tool for the GUI
    ///
    /// **Related Categories**: Settings
    DesktopSettings,

    /// A tool to manage hardware components, like sound cards, video cards or printers
    ///
    /// **Related Categories**: Settings
    HardwareSettings,

    /// A tool to manage printers
    ///
    /// **Related Categories**: HardwareSettings;Settings
    Printing,

    /// A package manager application
    ///
    /// **Related Categories**: Settings
    PackageManager,

    /// A dial-up program
    ///
    /// **Related Categories**: Network
    Dialup,

    /// An instant messaging client
    ///
    /// **Related Categories**: Network
    InstantMessaging,

    /// A chat client
    ///
    /// **Related Categories**: Network
    Chat,

    /// An IRC client
    ///
    /// **Related Categories**: Network
    IRCClient,

    /// RSS, podcast and other subscription based contents
    ///
    /// **Related Categories**: Network
    Feed,

    /// Tools like FTP or P2P programs
    ///
    /// **Related Categories**: Network
    FileTransfer,

    /// HAM radio software
    ///
    /// **Related Categories**: Network or Audio
    HamRadio,

    /// A news reader or a news ticker
    ///
    /// **Related Categories**: Network
    News,

    /// A P2P program
    ///
    /// **Related Categories**: Network
    P2P,

    /// A tool to remotely manage your PC
    ///
    /// **Related Categories**: Network
    RemoteAccess,

    /// Telephony via PC
    ///
    /// **Related Categories**: Network
    Telephony,

    /// Telephony tools, to dial a number, manage PBX, ...
    ///
    /// **Related Categories**: Utility
    TelephonyTools,

    /// Video Conference software
    ///
    /// **Related Categories**: Network
    VideoConference,

    /// A web browser
    ///
    /// **Related Categories**: Network
    WebBrowser,

    /// A tool for web developers
    ///
    /// **Related Categories**: Network or Development
    WebDevelopment,

    /// An app related to MIDI
    ///
    /// **Related Categories**: AudioVideo;Audio
    Midi,

    /// Just a mixer
    ///
    /// **Related Categories**: AudioVideo;Audio
    Mixer,

    /// A sequencer
    ///
    /// **Related Categories**: AudioVideo;Audio
    Sequencer,

    /// A tuner
    ///
    /// **Related Categories**: AudioVideo;Audio
    Tuner,

    /// A TV application
    ///
    /// **Related Categories**: AudioVideo;Video
    TV,

    /// Application to edit audio/video files
    ///
    /// **Related Categories**: Audio or Video or AudioVideo
    AudioVideoEditing,

    /// Application to play audio/video files
    ///
    /// **Related Categories**: Audio or Video or AudioVideo
    Player,

    /// Application to record audio/video files
    ///
    /// **Related Categories**: Audio or Video or AudioVideo
    Recorder,

    /// Application to burn a disc
    ///
    /// **Related Categories**: AudioVideo
    DiscBurning,

    /// An action game
    ///
    /// **Related Categories**: Game
    ActionGame,

    /// Adventure style game
    ///
    /// **Related Categories**: Game
    AdventureGame,

    /// Arcade style game
    ///
    /// **Related Categories**: Game
    ArcadeGame,

    /// A board game
    ///
    /// **Related Categories**: Game
    BoardGame,

    /// Falling blocks game
    ///
    /// **Related Categories**: Game
    BlocksGame,

    /// A card game
    ///
    /// **Related Categories**: Game
    CardGame,

    /// A game for kids
    ///
    /// **Related Categories**: Game
    KidsGame,

    /// Logic games like puzzles, etc
    ///
    /// **Related Categories**: Game
    LogicGame,

    /// A role playing game
    ///
    /// **Related Categories**: Game
    RolePlaying,

    /// A shooter game
    ///
    /// **Related Categories**: Game
    Shooter,

    /// A simulation game
    ///
    /// **Related Categories**: Game
    Simulation,

    /// A sports game
    ///
    /// **Related Categories**: Game
    SportsGame,

    /// A strategy game
    ///
    /// **Related Categories**: Game
    StrategyGame,

    /// A place to browse and install games or a launcher for a game or games
    ///
    /// **Related Categories**: Game
    LauncherStore,

    /// Companion apps for games, such as addon managers, remote play clients or other tools
    ///
    /// **Related Categories**: Game
    GameTool,

    /// Software to teach arts
    ///
    /// **Related Categories**: Education or Science
    Art,

    /// Education or Science
    Construction,

    /// Musical software
    ///
    /// **Related Categories**: AudioVideo or Education
    Music,

    /// Software to learn foreign languages
    ///
    /// **Related Categories**: Education or Science
    Languages,

    /// Artificial Intelligence software
    ///
    /// **Related Categories**: Education or Science
    ArtificialIntelligence,

    /// Astronomy software
    ///
    /// **Related Categories**: Education or Science
    Astronomy,

    /// Biology software
    ///
    /// **Related Categories**: Education or Science
    Biology,

    /// Chemistry software
    ///
    /// **Related Categories**: Education or Science
    Chemistry,

    /// Computer Science software
    ///
    /// **Related Categories**: Education or Science
    ComputerScience,

    /// Data visualization software
    ///
    /// **Related Categories**: Education or Science
    DataVisualization,

    /// Economy software
    ///
    /// **Related Categories**: Education or Science
    Economy,

    /// Electricity software
    ///
    /// **Related Categories**: Education or Science
    Electricity,

    /// Geography software
    ///
    /// **Related Categories**: Education or Science
    Geography,

    /// Geology software
    ///
    /// **Related Categories**: Education or Science
    Geology,

    /// Geoscience software, GIS
    ///
    /// **Related Categories**: Education or Science
    Geoscience,

    /// History software
    ///
    /// **Related Categories**: Education or Science
    History,

    /// Software for philosophy, psychology and other humanities
    ///
    /// **Related Categories**: Education or Science
    Humanities,

    /// Image Processing software
    ///
    /// **Related Categories**: Education or Science
    ImageProcessing,

    /// Literature software
    ///
    /// **Related Categories**: Education or Science
    Literature,

    /// Software for viewing maps, navigation, mapping, GPS
    ///
    /// **Related Categories**: Education or Science or Utility
    Maps,

    /// Math software
    ///
    /// **Related Categories**: Education or Science
    Math,

    /// Numerical analysis software
    ///
    /// **Related Categories**: Education;Math or Science;Math
    NumericalAnalysis,

    /// Medical software
    ///
    /// **Related Categories**: Education or Science
    MedicalSoftware,

    /// Physics software
    ///
    /// **Related Categories**: Education or Science
    Physics,

    /// Robotics software
    ///
    /// **Related Categories**: Education or Science
    Robotics,

    /// Religious and spiritual software, theology
    ///
    /// **Related Categories**: Education or Science or Utility
    Spirituality,

    /// Sports software
    ///
    /// **Related Categories**: Education or Science
    Sports,

    /// Parallel computing software
    ///
    /// **Related Categories**: Education;ComputerScience or Science;ComputerScience
    ParallelComputing,

    /// A simple amusement
    Amusement,

    /// A tool to archive/backup data
    ///
    /// **Related Categories**: Utility
    Archiving,

    /// A tool to manage compressed data/archives
    ///
    /// **Related Categories**: Utility;Archiving
    Compression,

    /// Electronics software, e.g. a circuit designer
    Electronics,

    /// Emulator of another platform, such as a DOS emulator
    ///
    /// **Related Categories**: System or Game
    Emulator,

    /// Engineering software, e.g. CAD programs
    Engineering,

    /// A file tool utility
    ///
    /// **Related Categories**: Utility or System
    FileTools,

    /// A file manager
    ///
    /// **Related Categories**: System;FileTools
    FileManager,

    /// A terminal emulator application
    ///
    /// **Related Categories**: System
    TerminalEmulator,

    /// A file system tool
    ///
    /// **Related Categories**: System
    Filesystem,

    /// Monitor application/applet that monitors some resource or activity
    ///
    /// **Related Categories**: System or Network
    Monitor,

    /// A security tool
    ///
    /// **Related Categories**: Settings or System
    Security,

    /// Accessibility
    ///
    /// **Related Categories**: Settings or Utility
    Accessibility,

    /// A calculator
    ///
    /// **Related Categories**: Utility
    Calculator,

    /// A clock application/applet
    ///
    /// **Related Categories**: Utility
    Clock,

    /// A text editor
    ///
    /// **Related Categories**: Utility
    TextEditor,

    /// Help or documentation
    Documentation,

    /// Application handles adult or explicit material
    Adult,

    /// Important application, core to the desktop such as a file manager or a help browser
    Core,

    /// Application based on KDE libraries
    ///
    /// **Related Categories**: QT
    Kde,

    /// Application based on COSMIC libraries
    Cosmic,

    /// Application based on GNOME libraries
    ///
    /// **Related Categories**: GTK
    Gnome,

    /// Application based on LXQt libraries
    ///
    /// **Related Categories**: QT
    LXQt,

    /// Application based on XFCE libraries
    ///
    /// **Related Categories**: GTK
    Xfce,

    /// Application based on DDE libraries
    ///
    /// **Related Categories**: Qt
    Dde,

    /// Application based on GTK+ libraries
    Gtk,

    /// Application based on Qt libraries
    Qt,

    /// Application based on Motif libraries
    Motif,

    /// Application based on Java GUI libraries, such as AWT or Swing
    Java,

    /// Application that only works inside a terminal (text-based or command line application)
    ConsoleOnly,
}

impl Display for AdditionalCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AdditionalCategory::TwoDGraphics => write!(f, "2DGraphics"),
            AdditionalCategory::ThreeDGraphics => write!(f, "3DGraphics"),
            AdditionalCategory::Ide => write!(f, "IDE"),
            AdditionalCategory::Pda => write!(f, "PDA"),
            AdditionalCategory::Ocr => write!(f, "OCR"),
            AdditionalCategory::Kde => write!(f, "KDE"),
            AdditionalCategory::Cosmic => write!(f, "COSMIC"),
            AdditionalCategory::Gnome => write!(f, "GNOME"),
            AdditionalCategory::Xfce => write!(f, "XFCE"),
            AdditionalCategory::Dde => write!(f, "DDE"),
            AdditionalCategory::Gtk => write!(f, "GTK"),
            _ => write!(f, "{:?}", self),
        }
    }
}

/// https://specifications.freedesktop.org/menu-spec/latest/reserved-category-registry.html
#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) enum ReservedCategory {
    /// A screen saver (launching this desktop entry should activate the screen saver)
    Screensaver,

    /// An application that is primarily an icon for the "system tray" or "notification area" (apps that open a normal window and just happen to have a tray icon as well should not list this category)
    TrayIcon,

    /// An applet that will run inside a panel or another such application, likely desktop specific
    Applet,

    /// A shell (an actual specific shell such as bash or tcsh, not a TerminalEmulator)
    Shell,
}

impl Display for ReservedCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) enum Category {
    Main(MainCategory),
    Additional(AdditionalCategory),
    Reserved(ReservedCategory),
}

impl Display for Category {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Category::Main(c) => write!(f, "{}", c),
            Category::Additional(c) => write!(f, "{}", c),
            Category::Reserved(c) => write!(f, "{}", c),
        }
    }
}
