import type { Port } from "./types";

/** A listener on loopback or on every address answers on localhost. */
export function hostOf(address: string): string {
  const local = ["127.0.0.1", "::1", "0.0.0.0", "::"].includes(address) || address.startsWith("127.");
  if (local) return "localhost";
  return address.includes(":") ? `[${address}]` : address;
}

export function urlOf(port: Port): string {
  return `http://${hostOf(port.address)}:${port.port}`;
}
