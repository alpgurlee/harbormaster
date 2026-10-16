export interface Plural {
  one: string;
  other: string;
}

export interface Messages {
  settings: string;
  servers: Plural;
  memoryOf: string;
  memoryInUse: string;
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
}

export const en: Messages = {
  settings: "Settings",
  servers: { one: "{count} server", other: "{count} servers" },
  memoryOf: "{used} of {total}",
  memoryInUse: "Memory in use",
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
};

export const tr: Messages = {
  settings: "Ayarlar",
  servers: { one: "{count} sunucu", other: "{count} sunucu" },
  memoryOf: "{used} / {total}",
  memoryInUse: "Kullanılan bellek",
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
};
