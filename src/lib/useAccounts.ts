import { useEffect, useRef, useState } from "react";
import { accountsStatus, askChatGpt, on, stopChatGpt } from "./bridge";
import type { AccountsStatus, ChatTurn } from "./types";

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
  pendingRef.current = pending;

  useEffect(() => {
    const sub = on("pill:chatgpt", (chunk) => {
      const current = pendingRef.current;
      if (!current || chunk.id !== current.id) return;
      if (chunk.kind === "delta") setPending({ ...current, text: current.text + chunk.text });
      if (chunk.kind === "error") setPending({ ...current, error: chunk.message, usage: chunk.usage });
      if (chunk.kind === "done") {
        if (current.text) setTurns((t) => [...t, { role: "assistant", text: current.text }]);
        setPending(null);
      }
    });
    return () => sub.off();
  }, []);

  const ask = (question: string) => {
    const text = question.trim();
    if (!text || (pending && !pending.error)) return;
    const next: ChatTurn[] = [...turns, { role: "user", text }];
    const id = crypto.randomUUID();
    setTurns(next);
    setPending({ id, text: "" });
    askChatGpt(id, next.slice(-CONTEXT_TURNS));
  };

  /** Stops the answer; what arrived so far is kept. */
  const stop = () => {
    if (!pending) return;
    stopChatGpt(pending.id);
    if (pending.text) setTurns((t) => [...t, { role: "assistant", text: pending.text }]);
    setPending(null);
  };

  const clear = () => {
    if (pending && !pending.error) stopChatGpt(pending.id);
    setTurns([]);
    setPending(null);
  };

  return { turns, pending, ask, stop, clear, busy: !!pending && !pending.error };
}

export type Chat = ReturnType<typeof useChat>;
