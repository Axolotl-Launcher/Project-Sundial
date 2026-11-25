// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

// A tiny MCP stdio client for GitNexus.
//
// The workflow needs structured tool results, but adding the MCP SDK to this
// repository would make a CI-only integration part of the application's
// dependency graph. GitNexus accepts newline-delimited JSON-RPC on stdio, so
// this client speaks just enough of the protocol to initialize, discover the
// tools, and call them.

import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";

interface JsonRpcResponse {
  readonly jsonrpc: "2.0";
  readonly id?: number;
  readonly result?: unknown;
  readonly error?: { readonly code: number; readonly message: string };
}

interface JsonRpcRequest {
  readonly jsonrpc: "2.0";
  readonly id: number;
  readonly method: string;
  readonly params: unknown;
}

export interface ToolContent {
  readonly type: string;
  readonly text?: string;
}

export interface ToolResult {
  readonly content?: readonly ToolContent[];
  readonly isError?: boolean;
}

const INITIALIZE_TIMEOUT_MS = 120_000;
const TOOL_TIMEOUT_MS = 180_000;
const BYTES_PER_KIB = 1024;
const KIB_PER_MIB = 1024;
const BYTES_PER_MIB = BYTES_PER_KIB * KIB_PER_MIB;
const MAX_BUFFER_MIB = 32;
const MAX_BUFFER_SIZE = MAX_BUFFER_MIB * BYTES_PER_MIB;
const STDERR_LIMIT = 20_000;
export const NEXT_STEP_SEPARATOR = "\n\n---\n**Next:**";

/// GitNexus appends a human-oriented "Next" hint after the JSON payload. It
/// is not part of the result, so remove it before parsing. A `Next` inside a
/// JSON string has no preceding blank line and separator, so it is safe.
export function stripNextStepHint(text: string): string {
  const hint = text.lastIndexOf(NEXT_STEP_SEPARATOR);
  return (hint < 0 ? text : text.slice(0, hint)).trim();
}

export class GitNexusClient {
  private readonly process: ChildProcessWithoutNullStreams;
  private readonly pending = new Map<number, {
    resolve: (value: unknown) => void;
    reject: (error: Error) => void;
  }>();
  private nextId = 1;
  private buffer = "";
  private stderr = "";
  private closed = false;

  constructor(
    command: string,
    args: readonly string[],
    cwd: string,
    env: NodeJS.ProcessEnv,
  ) {
    this.process = spawn(command, args, {
      cwd,
      env,
      stdio: ["pipe", "pipe", "pipe"],
      windowsHide: true,
    });
    this.process.stdout.setEncoding("utf8");
    this.process.stderr.setEncoding("utf8");
    this.process.stdout.on("data", (chunk: string) => {
      this.onData(chunk);
    });
    this.process.stderr.on("data", (chunk: string) => {
      this.stderr += chunk;
      if (this.stderr.length > STDERR_LIMIT) {
        this.stderr = this.stderr.slice(-STDERR_LIMIT);
      }
    });
    this.process.stdin.on("error", (error) => {
      this.fail(error);
    });
    this.process.on("error", (error) => {
      this.fail(error);
    });
    this.process.on("exit", (code, signal) => {
      this.fail(
        new Error(
          `GitNexus MCP exited (${String(code ?? signal ?? "unknown")}): ${this.stderr.trim()}`,
        ),
      );
    });
  }

  private onData(chunk: string): void {
    this.buffer += chunk;
    if (this.buffer.length > MAX_BUFFER_SIZE) {
      this.fail(new Error("GitNexus MCP response exceeded the buffer limit"));
      return;
    }
    for (;;) {
      const newline = this.buffer.indexOf("\n");
      if (newline < 0) {
        return;
      }
      const line = this.buffer.slice(0, newline).replace(/\r$/, "");
      this.buffer = this.buffer.slice(newline + 1);
      if (line.trim().length === 0) {
        continue;
      }
      this.onLine(line);
    }
  }

  private onLine(line: string): void {
    let message: JsonRpcResponse;
    try {
      message = JSON.parse(line) as JsonRpcResponse;
    } catch {
      return;
    }
    if (message.id === undefined) {
      return;
    }
    const pending = this.pending.get(message.id);
    if (pending === undefined) {
      return;
    }
    this.pending.delete(message.id);
    if (message.error !== undefined) {
      pending.reject(new Error(message.error.message));
    } else {
      pending.resolve(message.result);
    }
  }

  private fail(error: Error): void {
    if (this.closed) {
      return;
    }
    this.closed = true;
    for (const pending of this.pending.values()) {
      pending.reject(error);
    }
    this.pending.clear();
  }

  private send(request: Omit<JsonRpcRequest, "jsonrpc">, timeoutMs: number): Promise<unknown> {
    if (this.closed) {
      return Promise.reject(new Error(`GitNexus MCP is closed: ${this.stderr.trim()}`));
    }
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(request.id);
        reject(new Error(`GitNexus MCP timed out after ${String(timeoutMs)} ms`));
      }, timeoutMs);
      this.pending.set(request.id, {
        resolve: (value) => {
          clearTimeout(timer);
          resolve(value);
        },
        reject: (error) => {
          clearTimeout(timer);
          reject(error);
        },
      });
      this.process.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", ...request })}\n`);
    });
  }

  /// Initialize the MCP session. The initialized notification is required
  /// before the server will accept tool calls.
  async initialize(): Promise<void> {
    await this.send(
      {
        id: this.nextId++,
        method: "initialize",
        params: {
          protocolVersion: "2025-06-18",
          capabilities: {},
          clientInfo: { name: "axolotl-impact-report", version: "1" },
        },
      },
      INITIALIZE_TIMEOUT_MS,
    );
    this.process.stdin.write(
      `${JSON.stringify({ jsonrpc: "2.0", method: "notifications/initialized" })}\n`,
    );
  }

  async callTool(name: string, args: Readonly<Record<string, unknown>>): Promise<unknown> {
    const result = (await this.send(
      {
        id: this.nextId++,
        method: "tools/call",
        params: { name, arguments: args },
      },
      TOOL_TIMEOUT_MS,
    )) as ToolResult;
    if (result.isError === true) {
      const text = (result.content ?? [])
        .map((content) => content.text ?? "")
        .join("\n");
      throw new Error(text || `GitNexus tool "${name}" failed`);
    }
    const text = (result.content ?? [])
      .map((content) => content.text ?? "")
      .join("\n");
    if (text.length === 0) {
      return null;
    }
    const payload = stripNextStepHint(text);
    if (payload.length === 0) {
      return null;
    }
    try {
      return JSON.parse(payload) as unknown;
    } catch {
      return payload;
    }
  }

  close(): void {
    if (this.closed) {
      return;
    }
    this.closed = true;
    this.process.stdin.end();
    this.process.kill();
  }
}
