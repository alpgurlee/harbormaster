export interface Plural {
  one: string;
  other: string;
}

export interface Messages {
  settings: string;
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
}

export const en: Messages = {
  settings: "Settings",
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
};

export const tr: Messages = {
  settings: "Ayarlar",
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
};
