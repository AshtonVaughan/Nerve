/**
 * Browser- and Node-compatible Nerve client.
 *
 * Uses the global `WebSocket` constructor when present (browser, Bun, Deno),
 * otherwise falls back to the `ws` package. Detection happens once at
 * construction so tests can override it.
 */

import type {
  ActionEnvelope,
  ActionResult,
  AnyAction,
  AuditEntry,
  Capabilities,
  ClientMessage,
  ElementTarget,
  Observation,
  SafetyPolicy,
  ServerMessage,
} from "./types.js";

export class NerveClientError extends Error {
  constructor(public code: string, message: string) {
    super(`${code}: ${message}`);
  }
}

type WsLike = {
  send(data: string): void;
  close(code?: number, reason?: string): void;
  addEventListener(type: "open" | "close" | "error" | "message", cb: (ev: any) => void): void;
};

interface ClientOptions {
  host?: string;
  port?: number;
  clientName?: string;
  authToken?: string;
  autoReconnect?: boolean;
  reconnectInitialMs?: number;
  reconnectMaxMs?: number;
  webSocketFactory?: (url: string) => Promise<WsLike>;
}

async function defaultWsFactory(url: string): Promise<WsLike> {
  if (typeof WebSocket !== "undefined") {
    const ws = new WebSocket(url);
    await new Promise<void>((resolve, reject) => {
      ws.addEventListener("open", () => resolve(), { once: true });
      ws.addEventListener("error", (e: any) => reject(e), { once: true });
    });
    return ws as unknown as WsLike;
  }
  const mod = await import("ws");
  const Cls: any = (mod as any).WebSocket ?? (mod as any).default;
  const ws: any = new Cls(url);
  await new Promise<void>((resolve, reject) => {
    ws.once("open", () => resolve());
    ws.once("error", (err: any) => reject(err));
  });
  return {
    send: (data: string) => ws.send(data),
    close: (code?: number, reason?: string) => ws.close(code, reason),
    addEventListener: (type: any, cb: any) => {
      const map: Record<string, string> = {
        open: "open",
        close: "close",
        error: "error",
        message: "message",
      };
      if (type === "message") {
        ws.on(map[type], (data: any) => cb({ data: data.toString() }));
      } else {
        ws.on(map[type], (...args: any[]) => cb({ ...args }));
      }
    },
  };
}

export class NerveClient {
  private url: string;
  private clientName: string;
  private authToken: string | undefined;
  private autoReconnect: boolean;
  private reconnectInitialMs: number;
  private reconnectMaxMs: number;
  private webSocketFactory: (url: string) => Promise<WsLike>;
  private ws: WsLike | null = null;
  private pending = new Map<string, (msg: ServerMessage) => void>();
  private observers = new Set<(msg: ServerMessage) => void>();
  private hello: ServerMessage | null = null;
  private sessionId: string | null = null;
  private lastPolicy: SafetyPolicy | undefined;

  constructor(opts: ClientOptions = {}) {
    const host = opts.host ?? "127.0.0.1";
    const port = opts.port ?? 8765;
    this.url = `ws://${host}:${port}/`;
    this.clientName = opts.clientName ?? "nerve-typescript";
    const envToken =
      (typeof process !== "undefined" && (process as any).env?.NERVE_AUTH_TOKEN) || undefined;
    this.authToken = opts.authToken ?? envToken;
    this.autoReconnect = opts.autoReconnect ?? true;
    this.reconnectInitialMs = opts.reconnectInitialMs ?? 500;
    this.reconnectMaxMs = opts.reconnectMaxMs ?? 30_000;
    this.webSocketFactory = opts.webSocketFactory ?? defaultWsFactory;
  }

  async connect(policy?: SafetyPolicy): Promise<string> {
    this.lastPolicy = policy;
    let delay = this.reconnectInitialMs;
    for (let attempt = 0; ; attempt++) {
      try {
        return await this.connectOnce(policy);
      } catch (e) {
        if (!this.autoReconnect || attempt >= 8) {
          throw e;
        }
        await new Promise((res) => setTimeout(res, delay));
        delay = Math.min(delay * 2, this.reconnectMaxMs);
      }
    }
  }

  private async connectOnce(policy?: SafetyPolicy): Promise<string> {
    this.ws = await this.webSocketFactory(this.url);
    this.ws.addEventListener("message", (ev: any) => this.onMessage(String(ev.data)));
    this.ws.addEventListener("close", () => {
      for (const cb of this.pending.values()) {
        cb({ kind: "error", request_id: null, code: "closed", message: "websocket closed", retryable: true });
      }
      this.pending.clear();
    });

    this.hello = await this.waitForUnsolicited(
      (m) => m.kind === "hello"
    );
    const resp = await this.request<ServerMessage & { kind: "session_started" }>({
      kind: "session_start",
      client_name: this.clientName,
      client_version: "0.1.0",
      client_protocol_version: { major: 0, minor: 1, patch: 0 },
      auth_token: this.authToken,
      policy,
    } as any);
    this.sessionId = resp.session_id;
    return resp.session_id;
  }

  async stop(): Promise<void> {
    if (!this.ws) return;
    try {
      await this.request({ kind: "session_stop" } as ClientMessage);
    } catch {
      // ignore
    }
    this.ws.close();
    this.ws = null;
  }

  // -- Protocol methods --

  async getCapabilities(): Promise<Capabilities> {
    const resp = await this.request<ServerMessage & { kind: "capabilities" }>({
      kind: "get_capabilities",
    } as ClientMessage);
    return resp.capabilities;
  }

  async getObservation(options?: {
    includeScreenshot?: boolean;
    includeUiTree?: boolean;
    includeOcr?: boolean;
  }): Promise<Observation> {
    const resp = await this.request<ServerMessage & { kind: "observation" }>({
      kind: "get_observation",
      include_screenshot: options?.includeScreenshot ?? true,
      include_ui_tree: options?.includeUiTree ?? false,
      include_ocr: options?.includeOcr ?? false,
    } as ClientMessage);
    return resp.observation;
  }

  async *subscribeObservations(options?: {
    intervalMs?: number;
    includeScreenshot?: boolean;
    deltaFrames?: boolean;
  }): AsyncIterableIterator<Observation> {
    const requestId = this.newRequestId();
    const queue: Observation[] = [];
    let resolver: ((value: Observation | null) => void) | null = null;
    const handler = (msg: ServerMessage) => {
      if (msg.kind === "observation" && msg.request_id === requestId) {
        const obs = msg.observation;
        if (resolver) {
          const r = resolver;
          resolver = null;
          r(obs);
        } else {
          queue.push(obs);
        }
      }
    };
    this.observers.add(handler);
    try {
      this.sendRaw({
        kind: "subscribe_observations",
        request_id: requestId,
        interval_ms: options?.intervalMs ?? 500,
        include_screenshot: options?.includeScreenshot ?? false,
        delta_frames: options?.deltaFrames ?? false,
      } as any);
      while (true) {
        if (queue.length > 0) {
          yield queue.shift()!;
          continue;
        }
        const obs = await new Promise<Observation | null>((resolve) => {
          resolver = resolve;
        });
        if (!obs) break;
        yield obs;
      }
    } finally {
      this.observers.delete(handler);
    }
  }

  async *subscribeCursor(options?: {
    intervalMs?: number;
  }): AsyncIterableIterator<{
    timestamp: string;
    cursor: { x: number; y: number };
    activeWindow: string | null;
  }> {
    const requestId = this.newRequestId();
    const queue: any[] = [];
    let resolver: ((v: any) => void) | null = null;
    const handler = (msg: ServerMessage) => {
      if (msg.kind === "cursor_tick" && msg.request_id === requestId) {
        const tick = { timestamp: msg.timestamp, cursor: msg.cursor, activeWindow: msg.active_window };
        if (resolver) {
          const r = resolver;
          resolver = null;
          r(tick);
        } else {
          queue.push(tick);
        }
      }
    };
    this.observers.add(handler);
    try {
      this.sendRaw({
        kind: "subscribe_observations",
        request_id: requestId,
        interval_ms: options?.intervalMs ?? 16,
        include_screenshot: false,
        cursor_only: true,
      } as any);
      while (true) {
        if (queue.length > 0) {
          yield queue.shift()!;
          continue;
        }
        yield await new Promise((res) => (resolver = res));
      }
    } finally {
      this.observers.delete(handler);
    }
  }

  async execute(
    action: AnyAction,
    options?: { note?: string; idempotencyKey?: string },
  ): Promise<ActionResult> {
    const envelope: ActionEnvelope = {
      id: `act_${crypto.randomUUID()}`,
      action,
      note: options?.note,
      idempotency_key: options?.idempotencyKey,
    };
    const resp = await this.request<ServerMessage & { kind: "action_result" }>({
      kind: "execute_action",
      action: envelope,
    } as ClientMessage);
    return resp.result;
  }

  async executeBatch(actions: AnyAction[], stopOnError = true): Promise<ActionResult[]> {
    const envelopes: ActionEnvelope[] = actions.map((a) => ({
      id: `act_${crypto.randomUUID()}`,
      action: a,
    }));
    const resp = await this.request<ServerMessage & { kind: "batch_result" }>({
      kind: "execute_action_batch",
      actions: envelopes,
      stop_on_error: stopOnError,
    } as ClientMessage);
    return resp.results;
  }

  async getActionLog(options?: { sessionId?: string; limit?: number }): Promise<AuditEntry[]> {
    const resp = await this.request<ServerMessage & { kind: "action_log" }>({
      kind: "get_action_log",
      session_id: options?.sessionId,
      limit: options?.limit,
    } as ClientMessage);
    return resp.entries;
  }

  async setSafetyPolicy(policy: SafetyPolicy): Promise<SafetyPolicy> {
    const resp = await this.request<ServerMessage & { kind: "policy_updated" }>({
      kind: "set_safety_policy",
      policy,
    } as ClientMessage);
    return resp.policy;
  }

  async emergencyStop(): Promise<void> {
    await this.request({ kind: "emergency_stop" } as ClientMessage);
  }

  // -- Ergonomic helpers --

  click(x: number, y: number, button: "left" | "right" | "middle" = "left"): Promise<ActionResult> {
    return this.execute({ type: "click", x, y, button });
  }

  typeText(text: string, delayMs?: number): Promise<ActionResult> {
    return this.execute({ type: "type_text", text, delay_ms: delayMs ?? null });
  }

  hotkey(keys: string[]): Promise<ActionResult> {
    return this.execute({ type: "hotkey", keys });
  }

  async clipboardGet(): Promise<string> {
    const result = await this.execute({ type: "clipboard_get" });
    return (result.data?.text as string | undefined) ?? "";
  }

  clipboardSet(text: string): Promise<ActionResult> {
    return this.execute({ type: "clipboard_set", text });
  }

  clickElement(target: ElementTarget): Promise<ActionResult> {
    return this.execute({ type: "click_element", target });
  }

  openApp(name: string): Promise<ActionResult> {
    return this.execute({ type: "open_app", name });
  }

  // -- Internals --

  private newRequestId(): string {
    return `ts_${crypto.randomUUID()}`;
  }

  private sendRaw(payload: ClientMessage): void {
    if (!this.ws) throw new Error("client not connected");
    this.ws.send(JSON.stringify(payload));
  }

  private async request<T extends ServerMessage>(payload: Omit<ClientMessage, "request_id"> & { request_id?: string }): Promise<T> {
    const request_id = payload.request_id ?? this.newRequestId();
    const message = { ...payload, request_id } as ClientMessage;
    return new Promise<T>((resolve, reject) => {
      this.pending.set(request_id, (msg) => {
        if (msg.kind === "error") {
          reject(new NerveClientError(msg.code, msg.message));
        } else {
          resolve(msg as T);
        }
      });
      try {
        this.sendRaw(message);
      } catch (e) {
        this.pending.delete(request_id);
        reject(e);
      }
    });
  }

  private waitForUnsolicited(predicate: (m: ServerMessage) => boolean): Promise<ServerMessage> {
    return new Promise((resolve) => {
      const handler = (msg: ServerMessage) => {
        if (predicate(msg)) {
          this.observers.delete(handler);
          resolve(msg);
        }
      };
      this.observers.add(handler);
    });
  }

  private onMessage(data: string): void {
    let msg: ServerMessage;
    try {
      msg = JSON.parse(data) as ServerMessage;
    } catch {
      return;
    }
    const rid =
      (msg as { request_id?: string | null }).request_id !== undefined &&
      (msg as { request_id?: string | null }).request_id !== null
        ? (msg as { request_id?: string }).request_id
        : null;
    if (rid && this.pending.has(rid)) {
      const cb = this.pending.get(rid)!;
      this.pending.delete(rid);
      cb(msg);
      return;
    }
    for (const o of this.observers) {
      try {
        o(msg);
      } catch {
        // ignore
      }
    }
  }
}
