// Persistent child process pool for Rust CLI binaries.
// Keeps binaries running in --daemon mode, communicating via newline-delimited JSON.

import { spawn, ChildProcess } from "child_process";
import { createInterface, Interface } from "readline";

type PendingCall = {
  resolve: (value: any) => void;
  reject: (reason: any) => void;
  timer: ReturnType<typeof setTimeout>;
};

export class ProcessPool {
  private binPath: string;
  private args: string[];
  private child: ChildProcess | null = null;
  private rl: Interface | null = null;
  private queue: PendingCall[] = [];
  private pending: PendingCall | null = null;
  private timeout: number;

  constructor(binPath: string, args: string[] = ["--daemon"], timeout = 10000) {
    this.binPath = binPath;
    this.args = args;
    this.timeout = timeout;
  }

  private ensureProcess(): ChildProcess {
    if (this.child && this.child.exitCode === null) {
      return this.child;
    }

    this.child = spawn(this.binPath, this.args, {
      stdio: ["pipe", "pipe", "pipe"],
    });

    this.rl = createInterface({ input: this.child.stdout! });

    this.rl.on("line", (line: string) => {
      if (!this.pending) return;
      const { resolve, reject, timer } = this.pending;
      clearTimeout(timer);
      this.pending = null;
      try {
        const data = JSON.parse(line);
        if (data.error) {
          reject(new Error(data.error));
        } else {
          resolve(data);
        }
      } catch (e) {
        reject(new Error(`Invalid JSON from process: ${line}`));
      }
      this.drainQueue();
    });

    this.child.on("exit", () => {
      // Reject any pending call
      if (this.pending) {
        clearTimeout(this.pending.timer);
        this.pending.reject(new Error("Process exited unexpectedly"));
        this.pending = null;
      }
      // Reject all queued calls — they'll retry via ensureProcess on next call()
      for (const q of this.queue) {
        clearTimeout(q.timer);
        q.reject(new Error("Process exited unexpectedly"));
      }
      this.queue = [];
      this.child = null;
      this.rl = null;
    });

    this.child.stderr?.on("data", () => {
      // Discard stderr to prevent buffer filling up
    });

    return this.child;
  }

  private drainQueue() {
    if (this.pending || this.queue.length === 0) return;
    const next = this.queue.shift()!;
    this.pending = next;
    // The input was already written when queued — we just track the pending response
  }

  async call(input: Record<string, any>): Promise<any> {
    const proc = this.ensureProcess();
    const json = JSON.stringify(input) + "\n";

    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        if (this.pending?.resolve === resolve) {
          this.pending = null;
        }
        reject(new Error(`Process call timed out after ${this.timeout}ms`));
        this.drainQueue();
      }, this.timeout);

      const entry: PendingCall = { resolve, reject, timer };

      if (this.pending) {
        // Queue this call — write will happen when it becomes pending
        this.queue.push(entry);
        // Buffer the write so it's ready when the process reads
        proc.stdin!.write(json);
      } else {
        this.pending = entry;
        proc.stdin!.write(json);
      }
    });
  }

  shutdown() {
    if (this.child) {
      this.child.kill();
      this.child = null;
    }
    if (this.rl) {
      this.rl.close();
      this.rl = null;
    }
  }
}
