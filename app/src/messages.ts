export interface Plural {
  one: string;
  other: string;
}

export interface Messages {
  settings: string;
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
  processes: string;
  stopEllipsis: string;
  revealFolder: string;
  gone: string;
  wontStop: string;
  protectedSystem: string;
  protectedOtherUser: string;
  protectedSession: string;
  protectedUnverified: string;
  protectedDatabase: string;
  protectedSharedSocket: string;
  stopTitle: string;
  stopTitlePlain: string;
  checking: string;
  willStop: string;
  staysOpen: string;
  howItStops: string;
  cancel: string;
  stopServer: string;
  stopping: string;
  stillRunningAfter: string;
  forceStop: string;
  forcing: string;
  close: string;
  reasonRunsHarbormaster: string;
  reasonChanged: string;
  reasonExpired: string;
  reasonUntrackedExec: string;
  reasonUnknownLineage: string;
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
  groupLogs: string;
  groupTrash: string;
  aboutProjects: string;
  aboutPackages: string;
  aboutXcode: string;
  aboutApps: string;
  aboutLogs: string;
  aboutTrash: string;
  groupChosen: string;
  inUseBy: string;
  lastTouched: string;
  goesToTrash: string;
  emptiedForGood: string;
  usedLast: string;
  items: Plural;
  cleanEllipsis: string;
  confirmClean: string;
  confirmDeleted: Plural;
  confirmTrashed: Plural;
  confirmEmptied: string;
  confirmChecked: string;
  removing: string;
  freed: string;
  cleanedItems: Plural;
  leftItems: Plural;
  outcomeDeleted: string;
  outcomeTrashed: string;
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
  cleanClosed: string;
  cleanExpired: string;
  cleanUnlisted: string;
  overview: string;
  moduleDeveloper: string;
  moduleSystem: string;
  diskFree: string;
  diskOf: string;
  scan: string;
  scanHint: string;
  review: string;
  moduleSummary: string;
  searchItems: string;
  orderBy: string;
  orderSize: string;
  orderName: string;
  orderAge: string;
  chooseShown: string;
  revealInFinder: string;
  noItemsMatch: string;
}

export const en: Messages = {
  settings: "Settings",
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
  processes: "Processes",
  stopEllipsis: "Stop…",
  revealFolder: "Reveal folder",
  gone: "This server isn't running anymore.",
  wontStop: "Harbormaster won't stop this: {reason}.",
  protectedSystem: "it is part of the operating system",
  protectedOtherUser: "it runs as another user",
  protectedSession: "the port belongs to your shell, terminal or coding agent",
  protectedUnverified: "some of its processes couldn't be read, so it can't be checked",
  protectedDatabase: "it runs a database; stop that with the database's own tools",
  protectedSharedSocket: "another process holds its port too, so stopping it would not free the port",
  stopTitle: "Stop {name} on {ports}?",
  stopTitlePlain: "Stop {name}?",
  checking: "Checking…",
  willStop: "Will stop",
  staysOpen: "Stays open",
  howItStops: "Asks each process to quit, as Ctrl+C would. If something still runs after 5 seconds, you'll be asked before it's forced.",
  cancel: "Cancel",
  stopServer: "Stop server",
  stopping: "Stopping…",
  stillRunningAfter: "Still running after 5 seconds:",
  forceStop: "Force stop",
  forcing: "Forcing…",
  close: "Close",
  reasonRunsHarbormaster: "Harbormaster itself runs inside it.",
  reasonChanged: "The server changed since you looked. Look again before stopping it.",
  reasonExpired: "The confirmation lapsed. Try again.",
  reasonUntrackedExec: "This system can't tell when a process starts another program, so Harbormaster won't stop anything here.",
  reasonUnknownLineage: "Harbormaster couldn't trace what started it, so it stops nothing rather than risk stopping itself.",
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
  groupLogs: "Logs",
  groupTrash: "Trash",
  aboutProjects: "Build outputs of projects left alone for 30 days. npm install or cargo build makes them again.",
  aboutPackages: "Downloads your package managers keep. They come back when needed.",
  aboutXcode: "Build data and device symbols Xcode makes again. Archives are your own builds, so they go to the Trash.",
  aboutApps: "Caches apps fill as they run, and fill again.",
  aboutLogs: "What apps wrote down about their past runs.",
  aboutTrash: "What you threw away. Emptying it is for good.",
  groupChosen: "{chosen} of {count} chosen",
  inUseBy: "In use by {who}",
  lastTouched: "last touched {ago}",
  goesToTrash: "goes to the Trash",
  emptiedForGood: "emptied for good",
  usedLast: "used most recently",
  items: { one: "{count} item", other: "{count} items" },
  cleanEllipsis: "Clean up…",
  confirmClean: "Clean up {bytes}?",
  confirmDeleted: { one: "{count} item is deleted for good; its tool makes it again.", other: "{count} items are deleted for good; their tools make them again." },
  confirmTrashed: { one: "{count} item goes to the Trash, where you can put it back.", other: "{count} items go to the Trash, where you can put them back." },
  confirmEmptied: "The Trash is emptied for good.",
  confirmChecked: "Each item is checked again right before it goes; anything in use by then is left alone.",
  removing: "Cleaning up… {done} of {total}",
  freed: "{bytes} freed",
  cleanedItems: { one: "{count} item cleaned up", other: "{count} items cleaned up" },
  leftItems: { one: "{count} not cleaned up", other: "{count} not cleaned up" },
  outcomeDeleted: "deleted",
  outcomeTrashed: "moved to the Trash",
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
  cleanClosed: "The window was closed before the work could start.",
  cleanExpired: "This list is out of date. Scan again to clean up.",
  cleanUnlisted: "Something chosen wasn't in the list. Scan again to clean up.",
  overview: "Overview",
  moduleDeveloper: "Developer",
  moduleSystem: "System junk",
  diskFree: "{free} free of {total}",
  diskOf: "free of {total}",
  scan: "Scan",
  scanHint: "Looks for build outputs, package caches, Xcode data, app caches, logs and the Trash. Nothing goes without your say.",
  review: "Review",
  moduleSummary: "{found} found · {chosen} chosen",
  searchItems: "Search",
  orderBy: "Order",
  orderSize: "Largest first",
  orderName: "By name",
  orderAge: "Oldest first",
  chooseShown: "Choose all shown",
  revealInFinder: "Show in Finder",
  noItemsMatch: "Nothing here matches “{query}”.",
};

export const tr: Messages = {
  settings: "Ayarlar",
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
  processes: "Süreçler",
  stopEllipsis: "Durdur…",
  revealFolder: "Klasörü göster",
  gone: "Bu sunucu artık çalışmıyor.",
  wontStop: "Harbormaster bunu durdurmaz: {reason}.",
  protectedSystem: "işletim sisteminin bir parçası",
  protectedOtherUser: "başka bir kullanıcı olarak çalışıyor",
  protectedSession: "port kabuğunuza, terminalinize ya da kod ajanınıza ait",
  protectedUnverified: "bazı süreçleri okunamadı, bu yüzden denetlenemiyor",
  protectedDatabase: "bir veritabanı çalıştırıyor; onu veritabanının kendi araçlarıyla durdurun",
  protectedSharedSocket: "portunu başka bir süreç de tutuyor; durdurmak portu boşaltmaz",
  stopTitle: "{ports} üzerindeki {name} durdurulsun mu?",
  stopTitlePlain: "{name} durdurulsun mu?",
  checking: "Denetleniyor…",
  willStop: "Durdurulacak",
  staysOpen: "Açık kalacak",
  howItStops: "Her sürece, Ctrl+C gibi, kapanmasını söyler. 5 saniye sonra hâlâ çalışan olursa zorlamadan önce sorulur.",
  cancel: "Vazgeç",
  stopServer: "Sunucuyu durdur",
  stopping: "Durduruluyor…",
  stillRunningAfter: "5 saniye sonra hâlâ çalışıyor:",
  forceStop: "Zorla durdur",
  forcing: "Zorla durduruluyor…",
  close: "Kapat",
  reasonRunsHarbormaster: "Harbormaster'ın kendisi onun içinde çalışıyor.",
  reasonChanged: "Sunucu siz baktıktan sonra değişti. Durdurmadan önce yeniden bakın.",
  reasonExpired: "Onayın süresi doldu. Yeniden deneyin.",
  reasonUntrackedExec: "Bu sistem bir sürecin başka bir programa geçtiğini söyleyemiyor; bu yüzden Harbormaster burada hiçbir şeyi durdurmaz.",
  reasonUnknownLineage: "Harbormaster kendisini neyin başlattığını izleyemedi; kendine ya da başlatanına dokunmamak için hiçbir şeyi durdurmuyor.",
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
  groupLogs: "Kayıtlar",
  groupTrash: "Çöp Sepeti",
  aboutProjects: "30 gündür dokunulmayan projelerin derleme çıktıları. npm install ya da cargo build onları yeniden oluşturur.",
  aboutPackages: "Paket yöneticilerinin sakladığı indirmeler. Gerektiğinde yeniden iner.",
  aboutXcode: "Xcode'un yeniden oluşturduğu derleme verileri ve cihaz sembolleri. Arşivler senin derlemelerin; Çöp Sepeti'ne gider.",
  aboutApps: "Uygulamaların çalışırken doldurduğu ve yeniden dolduracağı önbellekler.",
  aboutLogs: "Uygulamaların geçmiş çalışmaları hakkında tuttuğu kayıtlar.",
  aboutTrash: "Attıkların. Boşaltmak kalıcıdır.",
  groupChosen: "{count} öğeden {chosen} seçili",
  inUseBy: "{who} kullanıyor",
  lastTouched: "son dokunuş {ago}",
  goesToTrash: "Çöp Sepeti'ne gider",
  emptiedForGood: "kalıcı olarak boşaltılır",
  usedLast: "en son kullanılan",
  items: { one: "{count} öğe", other: "{count} öğe" },
  cleanEllipsis: "Temizle…",
  confirmClean: "{bytes} temizlensin mi?",
  confirmDeleted: { one: "{count} öğe kalıcı olarak silinir; aracı onu yeniden oluşturur.", other: "{count} öğe kalıcı olarak silinir; araçları onları yeniden oluşturur." },
  confirmTrashed: { one: "{count} öğe Çöp Sepeti'ne gider; oradan geri alınabilir.", other: "{count} öğe Çöp Sepeti'ne gider; oradan geri alınabilir." },
  confirmEmptied: "Çöp Sepeti kalıcı olarak boşaltılır.",
  confirmChecked: "Her öğe gitmeden hemen önce yeniden denetlenir; o an kullanımda olan bırakılır.",
  removing: "Temizleniyor… {done}/{total}",
  freed: "{bytes} boşaldı",
  cleanedItems: { one: "{count} öğe temizlendi", other: "{count} öğe temizlendi" },
  leftItems: { one: "{count} öğe temizlenmedi", other: "{count} öğe temizlenmedi" },
  outcomeDeleted: "silindi",
  outcomeTrashed: "Çöp Sepeti'ne taşındı",
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
  cleanClosed: "İş başlamadan pencere kapandı.",
  cleanExpired: "Bu liste eskidi. Temizlemek için yeniden tara.",
  cleanUnlisted: "Seçilenlerden biri listede yoktu. Temizlemek için yeniden tara.",
  overview: "Genel Bakış",
  moduleDeveloper: "Geliştirici",
  moduleSystem: "Sistem çöpü",
  diskFree: "{total} içinde {free} boş",
  diskOf: "boş · toplam {total}",
  scan: "Tara",
  scanHint: "Derleme çıktıları, paket önbellekleri, Xcode verileri, uygulama önbellekleri, kayıtlar ve Çöp Sepeti aranır. Sen onaylamadan hiçbir şey silinmez.",
  review: "İncele",
  moduleSummary: "{found} bulundu · {chosen} seçili",
  searchItems: "Ara",
  orderBy: "Sırala",
  orderSize: "En büyük önce",
  orderName: "Ada göre",
  orderAge: "En eski önce",
  chooseShown: "Görünenlerin tümünü seç",
  revealInFinder: "Finder'da göster",
  noItemsMatch: "“{query}” ile eşleşen bir şey yok.",
};
