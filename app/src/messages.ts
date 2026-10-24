export interface Plural {
  one: string;
  other: string;
}

export interface Messages {
  settings: string;
  putBack: string;
  clean: string;
  servers: Plural;
  memoryOf: string;
  memoryInUse: string;
  memoryByApp: string;
  devServers: string;
  measuring: string;
  figuresMac: string;
  figuresWindows: string;
  cpu: string;
  search: string;
  otherListeners: string;
  noProject: string;
  worktree: string;
  port: string;
  updatedJustNow: string;
  updatedAt: string;
  looking: string;
  nothingRuns: string;
  nothingMatches: string;
  openInBrowser: string;
  stop: string;
  days: string;
  hours: string;
  minutes: string;
  underAMinute: string;
  back: string;
  about: string;
  running: string;
  openAt: string;
  copyUrl: string;
  copied: string;
  copyFailed: string;
  branch: string;
  folder: string;
  command: string;
  terminal: string;
  session: string;
  memory: string;
  memoryMac: string;
  memoryWindows: string;
  slowedByHeat: string;
  busiestServer: string;
  processes: string;
  stopEllipsis: string;
  stopNamed: string;
  forceQuit: string;
  forceQuitEllipsis: string;
  daemonWarning: string;
  daemonHow: string;
  daemonGone: string;
  daemonChanged: string;
  revealFolder: string;
  gone: string;
  wontStop: string;
  protectedSystem: string;
  protectedOtherUser: string;
  protectedOtherSession: string;
  protectedSession: string;
  protectedUnverified: string;
  protectedDatabase: string;
  protectedSharedSocket: string;
  stopTitle: string;
  stopTitlePlain: string;
  forceTitle: string;
  forceTitlePlain: string;
  checking: string;
  willStop: string;
  willEnd: string;
  staysOpen: string;
  howItStops: string;
  howItEnds: string;
  cancel: string;
  stopServer: string;
  stopping: string;
  ending: string;
  stillRunningAfter: string;
  forceStop: string;
  forcing: string;
  close: string;
  reasonRunsHarbormaster: string;
  reasonChanged: string;
  reasonExpired: string;
  reasonUntrackedExec: string;
  reasonUnknownLineage: string;
  reasonNotAllowed: string;
  reasonStrays: string;
  reasonFailed: string;
  outcomeStopped: string;
  outcomeAlreadyGone: string;
  outcomeReplaced: string;
  outcomeAccessDenied: string;
  outcomeStillRunning: string;
  outcomeFailed: string;
  quit: string;
  quitApp: string;
  quitTitle: string;
  quitHowApp: string;
  quitWarning: string;
  quitHowPrograms: string;
  willQuit: string;
  quitting: string;
  stillOpen: string;
  quitGone: string;
  quitChanged: string;
  wontQuit: string;
  closeApp: string;
  closeTitle: string;
  closeHowApp: string;
  closeWindows: string;
  closing: string;
  cleanLooking: string;
  cleanMeasured: string;
  cleanFoundSoFar: string;
  cleanChosen: string;
  cleanScanAgain: string;
  cleanNothing: string;
  groupProjects: string;
  groupPackages: string;
  groupXcode: string;
  groupApps: string;
  groupTemp: string;
  groupLogs: string;
  groupTrash: string;
  groupTrashWindows: string;
  groupVideos: string;
  groupArchives: string;
  groupInstallers: string;
  groupAudio: string;
  groupImages: string;
  groupDocuments: string;
  groupOtherFiles: string;
  aboutProjects: string;
  projectsChosen: Plural;
  aboutPackages: string;
  aboutXcode: string;
  aboutApps: string;
  aboutTemp: string;
  aboutLogs: string;
  aboutLogsWindows: string;
  aboutTrash: string;
  aboutVideos: string;
  aboutVideosWindows: string;
  aboutArchives: string;
  aboutInstallers: string;
  aboutInstallersWindows: string;
  aboutAudio: string;
  aboutImages: string;
  aboutDocuments: string;
  aboutOtherFiles: string;
  groupChosen: string;
  inUseBy: string;
  lastTouched: string;
  changedAgo: string;
  goesToTrash: string;
  goesToRecycleBin: string;
  emptiedForGood: string;
  usedLast: string;
  items: Plural;
  cleanEllipsis: string;
  confirmClean: string;
  confirmDeleted: Plural;
  confirmTrashed: Plural;
  confirmRecycled: Plural;
  confirmEmptied: string;
  confirmTemporary: string;
  confirmEmptiedRecycleBin: string;
  confirmChecked: string;
  removing: string;
  freed: string;
  freedSoFar: string;
  movedToTrash: string;
  movedToRecycleBin: string;
  trashHint: string;
  recycleBinHint: string;
  backToList: string;
  cleanedItems: Plural;
  leftItems: Plural;
  outcomeDeleted: string;
  outcomeTrashed: string;
  outcomePartly: Plural;
  outcomeRecycled: string;
  outcomeLeft: string;
  outcomeFailedWith: string;
  outcomeNotReached: string;
  outcomeInterrupted: string;
  whyOutsideHome: string;
  whyChanged: string;
  whyNotReached: string;
  whyInUse: string;
  whyUnchecked: string;
  cleanBusy: string;
  cleanExpired: string;
  cleanUnlisted: string;
  overview: string;
  moduleDeveloper: string;
  moduleSystem: string;
  moduleLarge: string;
  diskFree: string;
  diskOf: string;
  scan: string;
  scanHint: string;
  scanHintWindows: string;
  huntHint: string;
  huntHintWindows: string;
  huntLooking: string;
  huntMeasured: string;
  noLargeFiles: string;
  review: string;
  moduleSummary: string;
  searchItems: string;
  orderBy: string;
  orderSize: string;
  orderName: string;
  orderAge: string;
  chooseShown: string;
  heldHint: string;
  revealInFinder: string;
  revealInExplorer: string;
  noItemsMatch: string;
  moduleMap: string;
  mapStart: string;
  mapHint: string;
  mapHintWindows: string;
  mapOthers: string;
  mapOthersHint: string;
  mapLooking: string;
  mapCounted: Plural;
  mapAgain: string;
  mapHome: string;
  mapRest: string;
  mapFolded: string;
  mapFolder: string;
  mapFile: string;
  mapNothing: string;
  mapHolds: string;
  mapTrail: string;
  waitForMap: string;
  waitForCleaning: string;
  moduleApps: string;
  groupApplications: string;
  aboutApplications: string;
  appsHint: string;
  appsLooking: string;
  appsMeasured: string;
  noApps: string;
  appsStart: string;
  appsAgain: string;
  programsHint: string;
  programsLooking: string;
  noPrograms: string;
  aboutPrograms: string;
  programsEveryone: string;
  programsSize: string;
  programsUninstall: string;
  programsUninstallNamed: string;
  uninstallTitle: string;
  uninstallRuns: string;
  uninstallEveryone: string;
  uninstallYours: string;
  uninstallFollow: string;
  uninstallStart: string;
  uninstallStarting: string;
  uninstallStartedTitle: string;
  uninstallStarted: string;
  uninstallExpired: string;
  uninstallGone: string;
  uninstallChanged: string;
  uninstallUnsure: string;
  uninstallWantsAdministrator: string;
  uninstallUnprotected: string;
  uninstallDeclined: string;
  uninstallFailed: string;
  mapWithOthers: string;
  settingsGeneral: string;
  settingsServers: string;
  settingsCleanup: string;
  settingsAbout: string;
  language: string;
  sameAsSystem: string;
  appearance: string;
  appearanceLight: string;
  appearanceDark: string;
  hiddenTitle: string;
  hiddenHint: string;
  programName: string;
  add: string;
  removeEntry: string;
  rootsTitle: string;
  rootsHint: string;
  folderPath: string;
  idleTitle: string;
  aboutText: string;
  aboutTextWindows: string;
  aboutLicense: string;
  version: string;
  invalidIdleDays: string;
  invalidRoot: string;
  invalidTooMany: string;
  invalidName: string;
  invalidUnsaved: string;
  mapChanged: string;
  shortDeep: string;
  shortChanged: string;
  shortDenied: string;
  shortSkipped: string;
  shortStopped: string;
}

export const en: Messages = {
  settings: "Settings",
  putBack: "Put back by the icon",
  clean: "Clean up",
  servers: { one: "{count} server", other: "{count} servers" },
  memoryOf: "{used} of {total}",
  memoryInUse: "Memory in use",
  memoryByApp: "Memory by app",
  devServers: "Dev servers",
  measuring: "Measuring…",
  figuresMac: "Figures as in Activity Monitor",
  figuresWindows: "Figures as in Task Manager",
  cpu: "CPU {percent}",
  search: "Search servers, ports, projects",
  otherListeners: "Other listeners · {count}",
  noProject: "No project",
  worktree: "worktree",
  port: "Port {port}",
  updatedJustNow: "Updated just now",
  updatedAt: "Updated {time}",
  looking: "Looking for servers…",
  nothingRuns: "No dev servers are running.",
  nothingMatches: "Nothing matches “{query}”.",
  openInBrowser: "Open in browser",
  stop: "Stop",
  days: "{days}d {hours}h",
  hours: "{hours}h {minutes}m",
  minutes: "{minutes}m",
  underAMinute: "<1m",
  back: "Back",
  about: "About this server",
  running: "Running · {uptime}",
  openAt: "Open {host}:{port}",
  copyUrl: "Copy URL",
  copied: "Copied",
  copyFailed: "Couldn't copy",
  branch: "Branch",
  folder: "Folder",
  command: "Command",
  terminal: "Terminal",
  session: "Session",
  memory: "Memory",
  memoryMac: "as Activity Monitor counts it",
  memoryWindows: "as Task Manager counts it",
  slowedByHeat: "Your Mac is reducing performance to cool down.",
  busiestServer: "{name} is using {percent} CPU.",
  processes: "Processes",
  stopEllipsis: "Stop…",
  stopNamed: "Stop {name}",
  forceQuit: "Force quit",
  forceQuitEllipsis: "Force quit…",
  daemonWarning: "A build it is running now is cut short. Your next build starts a new one.",
  daemonHow: "It is asked to exit, as kill would, and never forced. Then the list looks again.",
  daemonGone: "Nothing to stop: no daemon alone keeps it in use any more.",
  daemonChanged: "What keeps it in use changed since you looked. Look again before stopping it.",
  revealFolder: "Reveal folder",
  gone: "This server isn't running anymore.",
  wontStop: "Harbormaster won't stop this: {reason}.",
  protectedSystem: "it is part of the operating system",
  protectedOtherUser: "it runs as another user",
  protectedOtherSession: "it runs in another of your sessions",
  protectedSession: "the port belongs to your shell, terminal or coding agent",
  protectedUnverified: "some of its processes couldn't be read, so it can't be checked",
  protectedDatabase: "it runs a database; stop that with the database's own tools",
  protectedSharedSocket: "another process holds its port too, so stopping it would not free the port",
  stopTitle: "Stop {name} on {ports}?",
  stopTitlePlain: "Stop {name}?",
  forceTitle: "Force quit {name} on {ports}?",
  forceTitlePlain: "Force quit {name}?",
  checking: "Checking…",
  willStop: "Will stop",
  willEnd: "Will end",
  staysOpen: "Stays open",
  howItStops: "Asks each process to quit, as Ctrl+C would. If something still runs after 5 seconds, you'll be asked before it's forced.",
  howItEnds:
    "Each process is ended at once, without being asked to quit, so it can't save or clean up first. Something outside this list may start it again.",
  cancel: "Cancel",
  stopServer: "Stop server",
  stopping: "Stopping…",
  ending: "Ending…",
  stillRunningAfter: "Still running after 5 seconds:",
  forceStop: "Force stop",
  forcing: "Forcing…",
  close: "Close",
  reasonRunsHarbormaster: "Harbormaster itself runs inside it.",
  reasonChanged: "The server changed since you looked. Look again before stopping it.",
  reasonExpired: "The confirmation lapsed. Try again.",
  reasonUntrackedExec: "This system can't tell when a process starts another program, so Harbormaster won't stop anything here.",
  reasonUnknownLineage: "Harbormaster couldn't trace what started it, so it stops nothing rather than risk stopping itself.",
  reasonNotAllowed:
    "Windows wouldn't let Harbormaster make sure of one of its processes, or end it. If it runs as administrator, stop it where it was started.",
  reasonStrays:
    "It started other programs that aren't part of it, such as another server, and on Windows those could end along with it. Stop them first.",
  reasonFailed: "Something went wrong: {message}",
  outcomeStopped: "stopped",
  outcomeAlreadyGone: "had already exited",
  outcomeReplaced: "became another process, so it was left alone",
  outcomeAccessDenied: "not allowed to stop it",
  outcomeStillRunning: "still running",
  outcomeFailed: "failed: {message}",
  quit: "Quit",
  quitApp: "Quit {name}",
  quitTitle: "Quit {name}?",
  quitHowApp: "{name} is asked to quit, as ⌘Q would. If something isn't saved, it asks you first.",
  quitWarning:
    "These are command-line programs. One may be a coding agent's session, a language server or a job running in the background; closing it can lose work in progress.",
  quitHowPrograms: "Each is asked to exit, as kill would. Nothing is forced.",
  willQuit: "Will close",
  quitting: "Quitting…",
  stillOpen: "{name} is still open. It may be waiting for you to save something.",
  quitGone: "{name} isn't running anymore.",
  quitChanged: "It changed since you looked. Look again before quitting it.",
  wontQuit: "Harbormaster won't quit this: {reason}.",
  closeApp: "Close {name}",
  closeTitle: "Close {name}?",
  closeHowApp:
    "Each of {name}'s windows is asked to close, as its close button would. It may ask you to save first, or stay open.",
  closeWindows: "Close windows",
  closing: "Closing…",
  cleanLooking: "Looking for projects and caches…",
  cleanMeasured: "Measured {done} of {total} places",
  cleanFoundSoFar: "{bytes} found so far",
  cleanChosen: "chosen to clean up, of {total} found",
  cleanScanAgain: "Scan again",
  cleanNothing: "Nothing to clean up.",
  groupProjects: "Projects",
  groupPackages: "Package caches",
  groupXcode: "Xcode",
  groupApps: "App caches",
  groupTemp: "Temporary files",
  groupLogs: "Logs",
  groupTrash: "Trash",
  groupTrashWindows: "Recycle Bin",
  groupVideos: "Videos",
  groupArchives: "Archives",
  groupInstallers: "Installers",
  groupAudio: "Audio",
  groupImages: "Images",
  groupDocuments: "Documents",
  groupOtherFiles: "Other files",
  aboutProjects: "Build outputs of your projects. npm install or cargo build makes them again.",
  projectsChosen: {
    one: "Chosen once their project has been left alone for {count} day.",
    other: "Chosen once their project has been left alone for {count} days.",
  },
  aboutPackages: "Downloads your package managers keep. They come back when needed.",
  aboutXcode: "Build data and device symbols Xcode makes again. Archives are your own builds, so they go to the Trash.",
  aboutApps: "Caches apps fill as they run, and fill again.",
  aboutTemp: "What apps put aside while they ran. Only what nothing has touched for a week goes, and none of what a program works in.",
  aboutLogs: "What apps wrote down about their past runs.",
  aboutLogsWindows: "What apps left behind when they crashed, kept to find out why. It goes to the Recycle Bin.",
  aboutTrash: "What you threw away. Emptying it is for good.",
  aboutVideos: "Films and screen recordings. What you choose goes to the Trash, where you can put it back.",
  aboutVideosWindows: "Films and screen recordings. What you choose goes to the Recycle Bin, where you can restore it.",
  aboutArchives: "Archives and disk images, often left behind once something was unpacked or installed.",
  aboutInstallers: "Installers and app packages, for this Mac or for phones and other systems.",
  aboutInstallersWindows: "Installers and app packages, for this PC or for phones and other systems.",
  aboutAudio: "Recordings and music files.",
  aboutImages: "Large pictures, such as camera raw files and layered designs.",
  aboutDocuments: "Large documents, presentations and spreadsheets.",
  aboutOtherFiles: "Large files of other kinds. Look at each before you choose it.",
  groupChosen: "{chosen} of {count} chosen",
  inUseBy: "In use by {who}",
  lastTouched: "last touched {ago}",
  changedAgo: "changed {ago}",
  goesToTrash: "goes to the Trash",
  goesToRecycleBin: "goes to the Recycle Bin",
  emptiedForGood: "emptied for good",
  usedLast: "used most recently",
  items: { one: "{count} item", other: "{count} items" },
  cleanEllipsis: "Clean up…",
  confirmClean: "Clean up {bytes}?",
  confirmDeleted: { one: "{count} item is deleted for good; its tool makes it again.", other: "{count} items are deleted for good; their tools make them again." },
  confirmTrashed: { one: "{count} item goes to the Trash, where you can put it back.", other: "{count} items go to the Trash, where you can put them back." },
  confirmRecycled: {
    one: "{count} item goes to the Recycle Bin, where you can restore it.",
    other: "{count} items go to the Recycle Bin, where you can restore them.",
  },
  confirmEmptied: "The Trash is emptied for good.",
  confirmTemporary: "What apps left in the temporary files a week or more ago is deleted for good.",
  confirmEmptiedRecycleBin: "The Recycle Bin is emptied for good.",
  confirmChecked: "Each item is checked again right before it goes; anything in use by then is left alone.",
  removing: "Cleaning up… {done} of {total}",
  freed: "{bytes} freed",
  freedSoFar: "{bytes} freed so far",
  movedToTrash: "{bytes} moved to the Trash",
  movedToRecycleBin: "{bytes} moved to the Recycle Bin",
  trashHint: "The room comes back once the Trash is emptied.",
  recycleBinHint: "The room comes back once the Recycle Bin is emptied.",
  backToList: "Back to the list",
  cleanedItems: { one: "{count} item cleaned up", other: "{count} items cleaned up" },
  leftItems: { one: "{count} not cleaned up", other: "{count} not cleaned up" },
  outcomeDeleted: "deleted",
  outcomeTrashed: "moved to the Trash",
  outcomePartly: {
    one: "deleted, all but 1 of its items, which was in use or changed meanwhile",
    other: "deleted, all but {count} of its items, which were in use or changed meanwhile",
  },
  outcomeRecycled: "moved to the Recycle Bin",
  outcomeLeft: "left alone: {why}",
  outcomeFailedWith: "failed: {why}",
  outcomeNotReached: "stopped before its turn",
  outcomeInterrupted: "stopped during cleanup; some contents may have been deleted",
  whyOutsideHome: "it isn't in your home folder",
  whyChanged: "another folder is in its place now",
  whyNotReached: "a link or something else stands in its way",
  whyInUse: "{who} is using it",
  whyUnchecked: "what is using it couldn't be read",
  cleanBusy: "A scan or clean-up is running already.",
  cleanExpired: "This list is out of date. Scan again to clean up.",
  cleanUnlisted: "Something chosen wasn't in the list. Scan again to clean up.",
  overview: "Overview",
  moduleDeveloper: "Developer",
  moduleSystem: "Leftovers",
  moduleLarge: "Large files",
  diskFree: "{free} free of {total}",
  diskOf: "free of {total}",
  scan: "Scan",
  scanHint: "Looks for build outputs, package caches, Xcode data, app caches, logs and the Trash. Nothing goes without your say.",
  scanHintWindows:
    "Looks for build outputs, the caches developer tools and apps keep, crash dumps and the Recycle Bin. Nothing goes without your say.",
  huntHint:
    "Looks through your folders outside the Library for files over 50 MB. None is chosen for you, and what you choose goes to the Trash. macOS may ask whether Harbormaster may look in Desktop, Documents and Downloads.",
  huntHintWindows:
    "Looks through your folders outside AppData for files over 50 MB. None is chosen for you, and what you choose goes to the Recycle Bin. Files OneDrive keeps in the cloud, and those File Explorer hides, are left out.",
  huntLooking: "Looking through your folders…",
  huntMeasured: "Looked through {done} of {total} folders",
  noLargeFiles: "No files over 50 MB were found in the folders looked in.",
  review: "Review",
  moduleSummary: "{found} found · {chosen} chosen",
  searchItems: "Search",
  orderBy: "Order",
  orderSize: "Largest first",
  orderName: "By name",
  orderAge: "Oldest first",
  chooseShown: "Choose all shown",
  heldHint: "Quit what is using an item, then come back here: the list looks again.",
  revealInFinder: "Show in Finder",
  revealInExplorer: "Show in File Explorer",
  noItemsMatch: "Nothing here matches “{query}”.",
  moduleMap: "Space map",
  mapStart: "Map",
  mapHint:
    "Shows where the room in your home folder goes, the Library and hidden folders too. Nothing is moved or deleted. macOS may ask whether Harbormaster may look in Desktop, Documents and Downloads.",
  mapHintWindows: "Shows where the room in your home folder goes, AppData and hidden folders too. Nothing is moved or deleted.",
  mapOthers: "Also look in other apps' data",
  mapOthersHint: "macOS asks before Harbormaster can look there.",
  mapLooking: "Mapping your home folder…",
  mapCounted: { one: "{items} item looked at", other: "{items} items looked at" },
  mapAgain: "Map again",
  mapHome: "Home",
  mapRest: "Smaller items",
  mapFolded: "Not shown one by one, to keep the map small.",
  mapFolder: "Folder",
  mapFile: "File",
  mapNothing: "Nothing here is large enough to show.",
  mapHolds: "What {name} holds",
  mapTrail: "Where you are",
  waitForMap: "The space map is being made. A scan can start once it is done or stopped.",
  waitForCleaning: "A scan or clean-up is running. The map can start once it is done.",
  moduleApps: "Apps",
  groupApplications: "Your apps",
  aboutApplications: "Apps that are yours, with the room each takes. What you choose goes to the Trash; its settings and data stay.",
  appsHint:
    "Lists the apps in Applications that are yours, with the room each takes. None is chosen for you, and what you choose goes to the Trash. Apps that aren't yours, such as those from the App Store, are left to Finder.",
  appsLooking: "Looking through your apps…",
  appsMeasured: "Measured {done} of {total} apps",
  noApps: "No apps of yours were found in Applications.",
  appsStart: "List apps",
  appsAgain: "Refresh list",
  programsHint: "Lists the apps installed on this PC, as Windows Settings does. Each is removed by its own uninstaller, which Harbormaster starts once you confirm; it never deletes an app's folder itself.",
  programsLooking: "Reading the list of installed apps…",
  noPrograms: "No installed apps were found.",
  aboutPrograms: "The apps installed on this PC, with the room their installers say they take. Each is removed by its own uninstaller, started only once you confirm.",
  programsEveryone: "for everyone on this PC",
  programsSize: "about {bytes}",
  programsUninstall: "Uninstall…",
  programsUninstallNamed: "Uninstall {name}…",
  uninstallTitle: "Uninstall {name}?",
  uninstallRuns: "Harbormaster starts the uninstaller it came with:",
  uninstallEveryone: "It's installed for everyone on this PC, so Windows may ask for an administrator. Allow it only if you trust the publisher Windows names.",
  uninstallYours: "It's installed for you alone, so Harbormaster starts its uninstaller as you and never as an administrator.",
  uninstallFollow: "The uninstaller decides what goes; follow its prompts. Harbormaster removes nothing itself.",
  uninstallStart: "Start uninstaller",
  uninstallStarting: "Starting…",
  uninstallStartedTitle: "{name}'s uninstaller was started",
  uninstallStarted: "Follow its prompts; Harbormaster hasn't checked that the app was removed. Once it is done, refresh the list.",
  uninstallExpired: "The list has changed since. Refresh it and try again.",
  uninstallGone: "{name} is no longer installed.",
  uninstallChanged: "How {name} is uninstalled changed since the list was made. Refresh it and try again.",
  uninstallUnsure: "Windows can't tell for sure which program uninstalls {name}, so Harbormaster won't start one. Remove it from Windows Settings.",
  uninstallWantsAdministrator: "Its uninstaller asks for an administrator, which an app installed for you alone shouldn't. If you trust it, remove it from Windows Settings.",
  uninstallUnprotected: "Its uninstaller asks for an administrator, yet isn't kept where only administrators can change it. If you trust it, remove it from Windows Settings.",
  uninstallDeclined: "Windows wasn't allowed to start the uninstaller as an administrator.",
  uninstallFailed: "The uninstaller couldn't be started: {message}",
  mapWithOthers: "Map with other apps' data",
  settingsGeneral: "General",
  settingsServers: "Servers",
  settingsCleanup: "Clean up",
  settingsAbout: "About",
  language: "Language",
  sameAsSystem: "Same as the system",
  appearance: "Appearance",
  appearanceLight: "Light",
  appearanceDark: "Dark",
  hiddenTitle: "Never listed as dev servers",
  hiddenHint: "Programs by the name the panel shows, such as node or python3. What they listen on is listed with the other listeners.",
  programName: "Program name",
  add: "Add",
  removeEntry: "Remove {name}",
  rootsTitle: "Folders with projects",
  rootsHint: "Where the cleaner looks for build outputs. With none, it looks in the usual folders, such as ~/code and ~/Developer. Each must be inside your home folder.",
  folderPath: "Folder, such as ~/code",
  idleTitle: "Days a project sits untouched before its build outputs are chosen",
  aboutText: "A menu bar app for the dev servers on your Mac, and a cleaner for what your tools leave behind.",
  aboutTextWindows: "A tray app for the dev servers on your PC, and a cleaner for what your tools leave behind.",
  aboutLicense: "Free and open source, under the MIT License.",
  version: "Version {version}",
  invalidIdleDays: "Days must be from 1 to 365.",
  invalidRoot: "{path} isn't a folder inside your home folder.",
  invalidTooMany: "That is more than 64.",
  invalidName: "“{name}” can't be a program's name.",
  invalidUnsaved: "The settings couldn't be saved: {message}",
  mapChanged: "It has changed since the map was made. Map again to see it as it is.",
  shortDeep: "Goes deeper than the map looks",
  shortChanged: "Something in it changed while it was mapped",
  shortDenied: "Some of it couldn't be read",
  shortSkipped: "Holds other apps' data, not looked in",
  shortStopped: "Not all of it was mapped before the stop",
};

export const tr: Messages = {
  settings: "Ayarlar",
  putBack: "Simgenin yanına geri koy",
  clean: "Temizle",
  servers: { one: "{count} sunucu", other: "{count} sunucu" },
  memoryOf: "{used} / {total}",
  memoryInUse: "Kullanılan bellek",
  memoryByApp: "Uygulamalara göre bellek",
  devServers: "Dev sunucuları",
  measuring: "Ölçülüyor…",
  figuresMac: "Değerler Etkinlik Monitörü'ndeki gibi",
  figuresWindows: "Değerler Görev Yöneticisi'ndeki gibi",
  cpu: "CPU {percent}",
  search: "Sunucu, port ya da proje ara",
  otherListeners: "Diğer dinleyiciler · {count}",
  noProject: "Projesiz",
  worktree: "worktree",
  port: "Port {port}",
  updatedJustNow: "Az önce güncellendi",
  updatedAt: "Son tarama {time}",
  looking: "Sunucular aranıyor…",
  nothingRuns: "Çalışan dev sunucusu yok.",
  nothingMatches: "“{query}” ile eşleşen bir şey yok.",
  openInBrowser: "Tarayıcıda aç",
  stop: "Durdur",
  days: "{days} g {hours} sa",
  hours: "{hours} sa {minutes} dk",
  minutes: "{minutes} dk",
  underAMinute: "<1 dk",
  back: "Geri",
  about: "Bu sunucu hakkında",
  running: "Çalışıyor · {uptime}",
  openAt: "Aç: {host}:{port}",
  copyUrl: "URL'yi kopyala",
  copied: "Kopyalandı",
  copyFailed: "Kopyalanamadı",
  branch: "Branch",
  folder: "Klasör",
  command: "Komut",
  terminal: "Terminal",
  session: "Oturum",
  memory: "Bellek",
  memoryMac: "Etkinlik Monitörü'ndeki gibi",
  memoryWindows: "Görev Yöneticisi'ndeki gibi",
  slowedByHeat: "Mac'in soğumak için performansını düşürüyor.",
  busiestServer: "{name}, {percent} CPU kullanıyor.",
  processes: "Süreçler",
  stopEllipsis: "Durdur…",
  stopNamed: "Durdur: {name}",
  forceQuit: "Zorla sonlandır",
  forceQuitEllipsis: "Zorla sonlandır…",
  daemonWarning: "Şu an çalıştırdığı bir derleme yarıda kesilir. Sonraki derlemen yenisini başlatır.",
  daemonHow: "kill gibi kapanması istenir, asla zorlanmaz. Ardından liste yeniden bakar.",
  daemonGone: "Durdurulacak bir şey yok: onu artık tek başına bir arka plan süreci kullanmıyor.",
  daemonChanged: "Baktığından beri onu kullanan değişti. Durdurmadan önce yeniden bak.",
  revealFolder: "Klasörü göster",
  gone: "Bu sunucu artık çalışmıyor.",
  wontStop: "Harbormaster bunu durdurmaz: {reason}.",
  protectedSystem: "işletim sisteminin bir parçası",
  protectedOtherUser: "başka bir kullanıcı olarak çalışıyor",
  protectedOtherSession: "senin başka bir oturumunda çalışıyor",
  protectedSession: "port kabuğunuza, terminalinize ya da kod ajanınıza ait",
  protectedUnverified: "bazı süreçleri okunamadı, bu yüzden denetlenemiyor",
  protectedDatabase: "bir veritabanı çalıştırıyor; onu veritabanının kendi araçlarıyla durdurun",
  protectedSharedSocket: "portunu başka bir süreç de tutuyor; durdurmak portu boşaltmaz",
  stopTitle: "{ports} üzerindeki {name} durdurulsun mu?",
  stopTitlePlain: "{name} durdurulsun mu?",
  forceTitle: "{ports} üzerindeki {name} zorla sonlandırılsın mı?",
  forceTitlePlain: "{name} zorla sonlandırılsın mı?",
  checking: "Denetleniyor…",
  willStop: "Durdurulacak",
  willEnd: "Sonlandırılacak",
  staysOpen: "Açık kalacak",
  howItStops: "Her sürece, Ctrl+C gibi, kapanmasını söyler. 5 saniye sonra hâlâ çalışan olursa zorlamadan önce sorulur.",
  howItEnds:
    "Her süreç, kapanması istenmeden hemen sonlandırılır; önce kaydedemez ya da toparlanamaz. Bu listenin dışındaki bir şey onu yeniden başlatabilir.",
  cancel: "Vazgeç",
  stopServer: "Sunucuyu durdur",
  stopping: "Durduruluyor…",
  ending: "Sonlandırılıyor…",
  stillRunningAfter: "5 saniye sonra hâlâ çalışıyor:",
  forceStop: "Zorla durdur",
  forcing: "Zorla durduruluyor…",
  close: "Kapat",
  reasonRunsHarbormaster: "Harbormaster'ın kendisi onun içinde çalışıyor.",
  reasonChanged: "Sunucu siz baktıktan sonra değişti. Durdurmadan önce yeniden bakın.",
  reasonExpired: "Onayın süresi doldu. Yeniden deneyin.",
  reasonUntrackedExec: "Bu sistem bir sürecin başka bir programa geçtiğini söyleyemiyor; bu yüzden Harbormaster burada hiçbir şeyi durdurmaz.",
  reasonUnknownLineage: "Harbormaster kendisini neyin başlattığını izleyemedi; kendine ya da başlatanına dokunmamak için hiçbir şeyi durdurmuyor.",
  reasonNotAllowed:
    "Windows, Harbormaster'ın süreçlerinden birini doğrulamasına ya da sonlandırmasına izin vermedi. Yönetici olarak çalışıyorsa başlatıldığı yerden durdurun.",
  reasonStrays:
    "Parçası olmayan başka programlar da başlatmış (örneğin başka bir sunucu); Windows'ta bunlar da onunla birlikte kapanabilir. Önce onları durdurun.",
  reasonFailed: "Bir şeyler ters gitti: {message}",
  outcomeStopped: "durduruldu",
  outcomeAlreadyGone: "zaten kapanmıştı",
  outcomeReplaced: "başka bir sürece dönüştü, dokunulmadı",
  outcomeAccessDenied: "durdurma izni yok",
  outcomeStillRunning: "hâlâ çalışıyor",
  outcomeFailed: "başarısız: {message}",
  quit: "Kapat",
  quitApp: "Kapat: {name}",
  quitTitle: "{name} kapatılsın mı?",
  quitHowApp: "{name}, ⌘Q'ya basılmış gibi kapanmaya çağrılır. Kaydedilmemiş bir şey varsa önce sana sorar.",
  quitWarning:
    "Bunlar komut satırı programları. Biri açık bir kod ajanı oturumu, dil sunucusu ya da arka planda süren bir iş olabilir; kapatınca yarım kalan iş kaybolabilir.",
  quitHowPrograms: "Her birine, kill komutu gibi, kapanması söylenir. Hiçbiri zorlanmaz.",
  willQuit: "Kapatılacak",
  quitting: "Kapatılıyor…",
  stillOpen: "{name} hâlâ açık. Bir şeyi kaydetmeni bekliyor olabilir.",
  quitGone: "{name} artık çalışmıyor.",
  quitChanged: "Sen baktıktan sonra değişti. Kapatmadan önce yeniden bak.",
  wontQuit: "Harbormaster bunu kapatmaz: {reason}.",
  closeApp: "Kapat: {name}",
  closeTitle: "{name} kapatılsın mı?",
  closeHowApp:
    "{name} uygulamasının her penceresine, kapat düğmesine basılmış gibi kapanması söylenir. Önce kaydetmek isteyip istemediğini sorabilir ya da açık kalabilir.",
  closeWindows: "Pencereleri kapat",
  closing: "Kapatılıyor…",
  cleanLooking: "Projeler ve önbellekler aranıyor…",
  cleanMeasured: "{total} yerden {done} tanesi ölçüldü",
  cleanFoundSoFar: "Şimdiye dek {bytes} bulundu",
  cleanChosen: "temizlemek için seçili; bulunan {total}",
  cleanScanAgain: "Yeniden tara",
  cleanNothing: "Temizlenecek bir şey yok.",
  groupProjects: "Projeler",
  groupPackages: "Paket önbellekleri",
  groupXcode: "Xcode",
  groupApps: "Uygulama önbellekleri",
  groupTemp: "Geçici dosyalar",
  groupLogs: "Kayıtlar",
  groupTrash: "Çöp Sepeti",
  groupTrashWindows: "Geri Dönüşüm Kutusu",
  groupVideos: "Videolar",
  groupArchives: "Arşivler",
  groupInstallers: "Yükleyiciler",
  groupAudio: "Ses",
  groupImages: "Görseller",
  groupDocuments: "Belgeler",
  groupOtherFiles: "Diğer dosyalar",
  aboutProjects: "Projelerinin derleme çıktıları. npm install ya da cargo build onları yeniden oluşturur.",
  projectsChosen: {
    one: "Projesine {count} gündür dokunulmadıysa seçili gelir.",
    other: "Projesine {count} gündür dokunulmadıysa seçili gelir.",
  },
  aboutPackages: "Paket yöneticilerinin sakladığı indirmeler. Gerektiğinde yeniden iner.",
  aboutXcode: "Xcode'un yeniden oluşturduğu derleme verileri ve cihaz sembolleri. Arşivler senin derlemelerin; Çöp Sepeti'ne gider.",
  aboutApps: "Uygulamaların çalışırken doldurduğu ve yeniden dolduracağı önbellekler.",
  aboutTemp: "Uygulamaların çalışırken bir kenara koydukları. Yalnızca bir haftadır dokunulmamış olanlar gider; bir programın içinde çalıştığı hiçbir şey gitmez.",
  aboutLogs: "Uygulamaların geçmiş çalışmaları hakkında tuttuğu kayıtlar.",
  aboutLogsWindows: "Uygulamaların çöktüklerinde geride bıraktıkları, nedenini bulmak için tutulan dökümler. Geri Dönüşüm Kutusu'na gider.",
  aboutTrash: "Attıkların. Boşaltmak kalıcıdır.",
  aboutVideos: "Filmler ve ekran kayıtları. Seçtiklerin Çöp Sepeti'ne gider; oradan geri alabilirsin.",
  aboutVideosWindows: "Filmler ve ekran kayıtları. Seçtiklerin Geri Dönüşüm Kutusu'na gider; oradan geri yükleyebilirsin.",
  aboutArchives: "Arşivler ve disk görüntüleri; çoğu bir şey açıldıktan ya da kurulduktan sonra geride kalır.",
  aboutInstallers: "Bu Mac ya da telefonlar ve başka sistemler için yükleyiciler ve uygulama paketleri.",
  aboutInstallersWindows: "Bu bilgisayar ya da telefonlar ve başka sistemler için yükleyiciler ve uygulama paketleri.",
  aboutAudio: "Kayıtlar ve müzik dosyaları.",
  aboutImages: "Kamera RAW dosyaları ve katmanlı tasarımlar gibi büyük görseller.",
  aboutDocuments: "Büyük belgeler, sunumlar ve tablolar.",
  aboutOtherFiles: "Başka türden büyük dosyalar. Seçmeden önce her birine bak.",
  groupChosen: "{count} öğeden {chosen} seçili",
  inUseBy: "{who} kullanıyor",
  lastTouched: "son dokunuş {ago}",
  changedAgo: "{ago} değişti",
  goesToTrash: "Çöp Sepeti'ne gider",
  goesToRecycleBin: "Geri Dönüşüm Kutusu'na gider",
  emptiedForGood: "kalıcı olarak boşaltılır",
  usedLast: "en son kullanılan",
  items: { one: "{count} öğe", other: "{count} öğe" },
  cleanEllipsis: "Temizle…",
  confirmClean: "{bytes} temizlensin mi?",
  confirmDeleted: { one: "{count} öğe kalıcı olarak silinir; aracı onu yeniden oluşturur.", other: "{count} öğe kalıcı olarak silinir; araçları onları yeniden oluşturur." },
  confirmTrashed: { one: "{count} öğe Çöp Sepeti'ne gider; oradan geri alınabilir.", other: "{count} öğe Çöp Sepeti'ne gider; oradan geri alınabilir." },
  confirmRecycled: {
    one: "{count} öğe Geri Dönüşüm Kutusu'na gider; oradan geri yüklenebilir.",
    other: "{count} öğe Geri Dönüşüm Kutusu'na gider; oradan geri yüklenebilir.",
  },
  confirmEmptied: "Çöp Sepeti kalıcı olarak boşaltılır.",
  confirmTemporary: "Geçici dosyalarda bir haftadan uzun süredir duranlar kalıcı olarak silinir.",
  confirmEmptiedRecycleBin: "Geri Dönüşüm Kutusu kalıcı olarak boşaltılır.",
  confirmChecked: "Her öğe gitmeden hemen önce yeniden denetlenir; o an kullanımda olan bırakılır.",
  removing: "Temizleniyor… {done}/{total}",
  freed: "{bytes} boşaldı",
  freedSoFar: "şimdilik {bytes} açıldı",
  movedToTrash: "{bytes} Çöp Sepeti'ne taşındı",
  movedToRecycleBin: "{bytes} Geri Dönüşüm Kutusu'na taşındı",
  trashHint: "Bu alan, Çöp Sepeti boşaltılınca açılır.",
  recycleBinHint: "Bu alan, Geri Dönüşüm Kutusu boşaltılınca açılır.",
  backToList: "Listeye dön",
  cleanedItems: { one: "{count} öğe temizlendi", other: "{count} öğe temizlendi" },
  leftItems: { one: "{count} öğe temizlenmedi", other: "{count} öğe temizlenmedi" },
  outcomeDeleted: "silindi",
  outcomeTrashed: "Çöp Sepeti'ne taşındı",
  outcomePartly: {
    one: "silindi; kullanımda olan ya da bu arada değişen 1 öğesi kaldı",
    other: "silindi; kullanımda olan ya da bu arada değişen {count} öğesi kaldı",
  },
  outcomeRecycled: "Geri Dönüşüm Kutusu'na taşındı",
  outcomeLeft: "bırakıldı: {why}",
  outcomeFailedWith: "başarısız: {why}",
  outcomeNotReached: "sırası gelmeden durduruldu",
  outcomeInterrupted: "temizlik sırasında durduruldu; içeriğin bir kısmı silinmiş olabilir",
  whyOutsideHome: "ana klasörünün içinde değil",
  whyChanged: "yerinde artık başka bir klasör var",
  whyNotReached: "yolunda bir bağlantı ya da başka bir şey var",
  whyInUse: "{who} kullanıyor",
  whyUnchecked: "onu neyin kullandığı okunamadı",
  cleanBusy: "Bir tarama ya da temizlik zaten sürüyor.",
  cleanExpired: "Bu liste eskidi. Temizlemek için yeniden tara.",
  cleanUnlisted: "Seçilenlerden biri listede yoktu. Temizlemek için yeniden tara.",
  overview: "Genel Bakış",
  moduleDeveloper: "Geliştirici",
  moduleSystem: "Artıklar",
  moduleLarge: "Büyük dosyalar",
  diskFree: "{total} içinde {free} boş",
  diskOf: "boş · toplam {total}",
  scan: "Tara",
  scanHint: "Derleme çıktıları, paket önbellekleri, Xcode verileri, uygulama önbellekleri, kayıtlar ve Çöp Sepeti aranır. Sen onaylamadan hiçbir şey silinmez.",
  scanHintWindows:
    "Derleme çıktıları, geliştirici araçlarının ve uygulamaların önbellekleri, çökme dökümleri ve Geri Dönüşüm Kutusu aranır. Sen onaylamadan hiçbir şey silinmez.",
  huntHint:
    "Library dışındaki klasörlerinde 50 MB'tan büyük dosyaları arar. Hiçbirini senin yerine seçmez; seçtiklerin Çöp Sepeti'ne gider. macOS, Masaüstü, Belgeler ve İndirilenler için izin isteyebilir.",
  huntHintWindows:
    "AppData dışındaki klasörlerinde 50 MB'tan büyük dosyaları arar. Hiçbirini senin yerine seçmez; seçtiklerin Geri Dönüşüm Kutusu'na gider. OneDrive'ın bulutta tuttuğu dosyalar ve Dosya Gezgini'nin gizledikleri aranmaz.",
  huntLooking: "Klasörlerin taranıyor…",
  huntMeasured: "{total} klasörden {done} tanesine bakıldı",
  noLargeFiles: "Bakılan klasörlerde 50 MB'tan büyük dosya bulunamadı.",
  review: "İncele",
  moduleSummary: "{found} bulundu · {chosen} seçili",
  searchItems: "Ara",
  orderBy: "Sırala",
  orderSize: "En büyük önce",
  orderName: "Ada göre",
  orderAge: "En eski önce",
  chooseShown: "Görünenlerin tümünü seç",
  heldHint: "Bir öğeyi kullananı kapat, sonra buraya dön; liste yeniden bakar.",
  revealInFinder: "Finder'da göster",
  revealInExplorer: "Dosya Gezgini'nde göster",
  noItemsMatch: "“{query}” ile eşleşen bir şey yok.",
  moduleMap: "Alan haritası",
  mapStart: "Haritala",
  mapHint:
    "Ana klasöründeki alanın nereye gittiğini, Library ve gizli klasörler dahil gösterir. Hiçbir şey taşınmaz ya da silinmez. macOS, Masaüstü, Belgeler ve İndirilenler için izin isteyebilir.",
  mapHintWindows: "Ana klasöründeki alanın nereye gittiğini, AppData ve gizli klasörler dahil gösterir. Hiçbir şey taşınmaz ya da silinmez.",
  mapOthers: "Diğer uygulamaların verilerine de bak",
  mapOthersHint: "macOS oraya bakılmadan önce izin ister.",
  mapLooking: "Ana klasörün haritalanıyor…",
  mapCounted: { one: "{items} öğeye bakıldı", other: "{items} öğeye bakıldı" },
  mapAgain: "Yeniden haritala",
  mapHome: "Ana klasör",
  mapRest: "Daha küçük öğeler",
  mapFolded: "Haritayı küçük tutmak için tek tek gösterilmiyor.",
  mapFolder: "Klasör",
  mapFile: "Dosya",
  mapNothing: "Burada gösterilecek kadar büyük bir şey yok.",
  mapHolds: "{name} içinde olanlar",
  mapTrail: "Bulunduğun yer",
  waitForMap: "Alan haritası çıkarılıyor. Tarama, harita bitince ya da durdurulunca başlayabilir.",
  waitForCleaning: "Bir tarama ya da temizlik sürüyor. Harita, o bitince başlayabilir.",
  moduleApps: "Uygulamalar",
  groupApplications: "Uygulamaların",
  aboutApplications: "Sana ait uygulamalar ve kapladıkları alan. Seçtiklerin Çöp Sepeti'ne gider; ayarları ve verileri yerinde kalır.",
  appsHint:
    "Uygulamalar klasöründeki sana ait uygulamaları kapladıkları alanla listeler. Hiçbirini senin yerine seçmez; seçtiklerin Çöp Sepeti'ne gider. App Store'dan gelenler gibi sana ait olmayan uygulamalar Finder'a bırakılır.",
  appsLooking: "Uygulamaların inceleniyor…",
  appsMeasured: "{total} uygulamadan {done} tanesi ölçüldü",
  noApps: "Uygulamalar klasöründe sana ait bir uygulama bulunamadı.",
  appsStart: "Uygulamaları listele",
  appsAgain: "Listeyi yenile",
  programsHint: "Bu bilgisayarda yüklü uygulamaları Windows Ayarları gibi listeler. Her biri kendi kaldırıcısıyla kaldırılır; Harbormaster onu sen onaylayınca başlatır ve bir uygulamanın klasörünü asla kendisi silmez.",
  programsLooking: "Yüklü uygulamaların listesi okunuyor…",
  noPrograms: "Yüklü bir uygulama bulunamadı.",
  aboutPrograms: "Bu bilgisayarda yüklü uygulamalar ve yükleyicilerine göre kapladıkları alan. Her biri kendi kaldırıcısıyla kaldırılır; kaldırıcı yalnızca sen onaylayınca başlatılır.",
  programsEveryone: "bu bilgisayardaki herkes için",
  programsSize: "yaklaşık {bytes}",
  programsUninstall: "Kaldır…",
  programsUninstallNamed: "{name} uygulamasını kaldır…",
  uninstallTitle: "{name} kaldırılsın mı?",
  uninstallRuns: "Harbormaster, uygulamayla gelen kaldırıcıyı başlatır:",
  uninstallEveryone: "Bilgisayardaki herkes için yüklü, bu yüzden Windows yönetici izni isteyebilir. Yalnızca Windows'un gösterdiği yayıncıya güveniyorsan izin ver.",
  uninstallYours: "Yalnızca senin için yüklü; bu yüzden Harbormaster kaldırıcısını senin adına başlatır, asla yönetici olarak başlatmaz.",
  uninstallFollow: "Neyin kaldırılacağına kaldırıcı karar verir; onun adımlarını izle. Harbormaster kendisi hiçbir şey silmez.",
  uninstallStart: "Kaldırıcıyı başlat",
  uninstallStarting: "Başlatılıyor…",
  uninstallStartedTitle: "{name} kaldırıcısı başlatıldı",
  uninstallStarted: "Adımlarını izle; Harbormaster uygulamanın kaldırıldığını denetlemedi. Bitince listeyi yenile.",
  uninstallExpired: "Liste o zamandan beri değişti. Yenileyip yeniden dene.",
  uninstallGone: "{name} artık yüklü değil.",
  uninstallChanged: "{name} uygulamasının nasıl kaldırılacağı liste çıkarıldıktan sonra değişti. Listeyi yenileyip yeniden dene.",
  uninstallUnsure: "Windows, {name} uygulamasını hangi programın kaldırdığını kesin bilemiyor; bu yüzden Harbormaster bir program başlatmaz. Windows Ayarları'ndan kaldır.",
  uninstallWantsAdministrator: "Kaldırıcısı yönetici izni istiyor; yalnızca senin için yüklü bir uygulama bunu istememeli. Güveniyorsan Windows Ayarları'ndan kaldır.",
  uninstallUnprotected: "Kaldırıcısı yönetici izni istiyor, ama yalnızca yöneticilerin değiştirebileceği bir yerde durmuyor. Güveniyorsan Windows Ayarları'ndan kaldır.",
  uninstallDeclined: "Windows'un kaldırıcıyı yönetici olarak başlatmasına izin verilmedi.",
  uninstallFailed: "Kaldırıcı başlatılamadı: {message}",
  mapWithOthers: "Uygulama verileriyle haritala",
  settingsGeneral: "Genel",
  settingsServers: "Sunucular",
  settingsCleanup: "Temizlik",
  settingsAbout: "Hakkında",
  language: "Dil",
  sameAsSystem: "Sistemle aynı",
  appearance: "Görünüm",
  appearanceLight: "Açık",
  appearanceDark: "Koyu",
  hiddenTitle: "Dev sunucusu sayılmayanlar",
  hiddenHint: "Panelin gösterdiği adıyla programlar, örneğin node ya da python3. Dinledikleri portlar diğer dinleyicilerle birlikte listelenir.",
  programName: "Program adı",
  add: "Ekle",
  removeEntry: "{name} kaldır",
  rootsTitle: "Projelerin olduğu klasörler",
  rootsHint: "Temizleyicinin derleme çıktılarını aradığı yerler. Hiç yoksa ~/code ve ~/Developer gibi alışılmış klasörlere bakar. Her biri ana klasörünün içinde olmalı.",
  folderPath: "Klasör, örneğin ~/code",
  idleTitle: "Derleme çıktıları seçilmeden önce bir projenin dokunulmadan kalacağı gün",
  aboutText: "Mac'indeki dev sunucuları için bir menü çubuğu uygulaması ve araçlarının geride bıraktıkları için bir temizleyici.",
  aboutTextWindows: "Bilgisayarındaki dev sunucuları için bir sistem tepsisi uygulaması ve araçlarının geride bıraktıkları için bir temizleyici.",
  aboutLicense: "Ücretsiz ve açık kaynak; MIT Lisansı ile.",
  version: "Sürüm {version}",
  invalidIdleDays: "Gün sayısı 1 ile 365 arasında olmalı.",
  invalidRoot: "{path}, ana klasörünün içinde bir klasör değil.",
  invalidTooMany: "Bu, 64'ten fazla.",
  invalidName: "“{name}” bir program adı olamaz.",
  invalidUnsaved: "Ayarlar kaydedilemedi: {message}",
  mapChanged: "Harita çıkarıldıktan sonra değişmiş. Güncel halini görmek için yeniden haritala.",
  shortDeep: "Haritanın baktığından daha derine iniyor",
  shortChanged: "Haritalanırken içinde bir şey değişti",
  shortDenied: "Bir kısmı okunamadı",
  shortSkipped: "Diğer uygulamaların verisini içeriyor; bakılmadı",
  shortStopped: "Durdurulmadan önce tamamı haritalanamadı",
};
