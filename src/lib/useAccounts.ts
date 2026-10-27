import { useEffect, useRef, useState } from "react";
import { accountsStatus, askChatGpt, on, stopChatGpt } from "./bridge";
import type { AccountsStatus, ChatTurn, Screenshot } from "./types";

const EMPTY: AccountsStatus = {
  google: { connected: false, busy: false, planUsage: false },
  chatgpt: { connected: false, busy: false, planUsage: false },
};

/** Google and ChatGPT connection status, kept current by `accounts:changed`. */
export function useAccounts(): AccountsStatus {
  const [status, setStatus] = useState<AccountsStatus>(EMPTY);
  useEffect(() => {
    const sub = on("accounts:changed", setStatus);
    accountsStatus().then(setStatus, console.error);
    return () => sub.off();
  }, []);
  return status;
}

/** Earlier turns sent with each question, so follow-ups make sense. Kept short: it's a notch. */
const CONTEXT_TURNS = 8;

export type Pending = { id: string; text: string; error?: string; usage?: boolean };

/**
 * The conversation in the notch's ChatGPT tab. Lives above the tab so it survives the notch
 * closing; it isn't saved anywhere, and OpenAI is asked not to store it either.
 */
export function useChat() {
  const [turns, setTurns] = useState<ChatTurn[]>([]);
  const [pending, setPending] = useState<Pending | null>(null);
  const pendingRef = useRef<Pending | null>(null);
  // Pieces of an answer often arrive several to a frame, before React re-renders, so each one
  // builds on this ref, updated here and now, not on the last render's copy (which would drop
  // every piece but the last of each frame).
  const update = (next: Pending | null) => {
    pendingRef.current = next;
    setPending(next);
  };

  useEffect(() => {
    const sub = on("pill:chatgpt", (chunk) => {
      const current = pendingRef.current;
      if (!current || chunk.id !== current.id) return;
      if (chunk.kind === "delta") update({ ...current, text: current.text + chunk.text });
      if (chunk.kind === "error") update({ ...current, error: chunk.message, usage: chunk.usage });
      if (chunk.kind === "done") {
        if (current.text) setTurns((t) => [...t, { role: "assistant", text: current.text }]);
        update(null);
      }
    });
    return () => sub.off();
  }, []);

  /** `shot`: a captured screen to send with the question. */
  const ask = (question: string, shot?: Screenshot) => {
    // A screen on its own is a question too.
    const text = question.trim() || (shot ? "Help me with what's on my screen." : "");
    if (!text || (pending && !pending.error)) return;
    const next: ChatTurn[] = [...turns, { role: "user", text, ...(shot ? { screen: shot.id, preview: shot.preview } : {}) }];
    const id = crypto.randomUUID();
    setTurns(next);
    update({ id, text: "" });
    askChatGpt(id, next.slice(-CONTEXT_TURNS));
  };

  /** Stops the answer; what arrived so far is kept. */
  const stop = () => {
    if (!pending) return;
    stopChatGpt(pending.id);
    if (pending.text) setTurns((t) => [...t, { role: "assistant", text: pending.text }]);
    update(null);
  };

  const clear = () => {
    if (pending && !pending.error) stopChatGpt(pending.id);
    setTurns([]);
    update(null);
  };

  return { turns, pending, ask, stop, clear, busy: !!pending && !pending.error };
}

export type Chat = ReturnType<typeof useChat>;
