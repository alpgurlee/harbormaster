import type { Port } from "./types";

/** A listener on loopback or on every address answers on localhost. */
export function hostOf(address: string): string {
  const local = ["127.0.0.1", "::1", "0.0.0.0", "::"].includes(address) || address.startsWith("127.");
  if (local) return "localhost";
  return address.includes(":") ? `[${address}]` : address;
}

/** fe80::/10 answers only on the interface its listener named, which isn't known here. */
function linkLocal(address: string): boolean {
  const first = address.split(":")[0];
  return address.includes(":") && first !== "" && (Number.parseInt(first, 16) & 0xffc0) === 0xfe80;
}

/** Where a listener answers, or null where no URL can reach it. */
export function urlOf(port: Port): string | null {
  if (linkLocal(port.address)) return null;
  return `http://${hostOf(port.address)}:${port.port}`;
}
