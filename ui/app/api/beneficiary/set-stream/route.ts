// GET /api/beneficiary/set-stream — SSE stream for anonymity set status
//
// Bridges the gRPC SubscribeSetFinalization stream to the browser via SSE.
// Sends periodic status updates (count/capacity) and a final "finalized" event.

import { getRegistryClient, grpcCall } from "@/lib/grpc";
import { getState } from "@/lib/state";

export const dynamic = "force-dynamic";

export async function GET() {
  const state = getState();

  const encoder = new TextEncoder();
  let closed = false;

  const stream = new ReadableStream({
    async start(controller) {
      const send = (event: string, data: Record<string, unknown>) => {
        if (closed) return;
        try {
          controller.enqueue(encoder.encode(`event: ${event}\ndata: ${JSON.stringify(data)}\n\n`));
        } catch {
          closed = true;
        }
      };

      // Poll set status and push updates
      const pollStatus = async () => {
        try {
          const currentState = getState();
          const setIdBuf = currentState.set_id_bytes
            ? Buffer.from(currentState.set_id_bytes, "hex")
            : Buffer.alloc(32);
          const resp: any = await grpcCall(getRegistryClient(), "GetAnonymitySet", { set_id: setIdBuf });
          send("status", {
            count: resp.count,
            capacity: resp.capacity,
            finalized: resp.finalized,
          });
          if (resp.finalized) {
            const commitments = (resp.commitments || []).map((c: Buffer) =>
              Buffer.from(c).toString("hex")
            );
            send("finalized", {
              count: resp.count,
              capacity: resp.capacity,
              finalized: true,
              commitments,
            });
            return true; // done
          }
        } catch {
          // registry not ready yet
        }
        return false;
      };

      // Initial status
      const done = await pollStatus();
      if (done) {
        controller.close();
        return;
      }

      // Poll every 2 seconds until finalized or client disconnects
      const interval = setInterval(async () => {
        if (closed) {
          clearInterval(interval);
          return;
        }
        const done = await pollStatus();
        if (done) {
          clearInterval(interval);
          try { controller.close(); } catch { /* already closed */ }
        }
      }, 2000);

      // Clean up if the client disconnects (AbortSignal not available in
      // Next.js route handlers, so we rely on the write error in send())
    },
    cancel() {
      closed = true;
    },
  });

  return new Response(stream, {
    headers: {
      "Content-Type": "text/event-stream",
      "Cache-Control": "no-cache, no-transform",
      Connection: "keep-alive",
    },
  });
}
