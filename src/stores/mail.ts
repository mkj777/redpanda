import { create } from "zustand";
import {
  api,
  type Account,
  type Draft,
  type Folder,
  type MessageSummary,
  type Subscription,
  type SyncState,
  type UnifiedCounts,
  type View,
} from "@/lib/ipc";
import { useSettings } from "./settings";

const PAGE = 100;

interface MailState {
  accounts: Account[];
  folders: Folder[];
  unified: UnifiedCounts;
  drafts: Draft[];
  view: View;
  /** The view to return to when the search box is cleared. */
  lastView: View;
  messages: MessageSummary[];
  hasMore: boolean;
  /** Older mail may still be on the server (folder views only). */
  serverHasOlder: boolean;
  loading: boolean;
  loadingOlder: boolean;
  selectedId: number | null;
  /** Newsletter senders, in the subscriptions view. */
  subscriptions: Subscription[];
  /** The selected sender there, see `subscriptionKey`. */
  selectedSub: string | null;
  /** Bumped on every refresh so open readers can refetch. */
  revision: number;

  refreshAccounts: () => Promise<void>;
  refreshFolders: () => Promise<void>;
  refreshList: () => Promise<void>;
  refreshAll: () => Promise<void>;
  setAccountStatus: (accountId: string, state: SyncState, error: string | null) => void;
  setView: (view: View) => void;
  search: (query: string) => void;
  loadMore: () => Promise<void>;
  select: (id: number | null) => void;
  selectSub: (key: string | null) => void;
  selectRelative: (delta: number) => void;
  /** Removes rows from the list right away and moves the selection on. */
  dropFromList: (ids: number[]) => void;
}

const emptyCounts: UnifiedCounts = { inboxUnread: 0, unread: 0, starred: 0, drafts: 0 };

function sameView(a: View, b: View) {
  return JSON.stringify(a) === JSON.stringify(b);
}

let listToken = 0;

export function subscriptionKey(s: Pick<Subscription, "accountId" | "sender">) {
  return `${s.accountId}:${s.sender.email.toLowerCase()}`;
}

export const useMail = create<MailState>()((set, get) => ({
  accounts: [],
  folders: [],
  unified: emptyCounts,
  drafts: [],
  view: { kind: "unified", which: "inbox" },
  lastView: { kind: "unified", which: "inbox" },
  messages: [],
  hasMore: false,
  serverHasOlder: true,
  loading: false,
  loadingOlder: false,
  selectedId: null,
  subscriptions: [],
  selectedSub: null,
  revision: 0,

  refreshAccounts: async () => {
    set({ accounts: await api.listAccounts() });
  },

  refreshFolders: async () => {
    const { folders, unified } = await api.listFolders();
    set({ folders, unified });
  },

  refreshList: async () => {
    const { view, messages } = get();
    const token = ++listToken;
    if (view.kind === "drafts") {
      const drafts = await api.listDrafts();
      if (token === listToken) set({ drafts, loading: false });
      return;
    }
    if (view.kind === "subscriptions") {
      const subscriptions = await api.listSubscriptions();
      if (token !== listToken) return;
      const { selectedSub } = get();
      set((s) => ({
        subscriptions,
        loading: false,
        selectedSub: subscriptions.some((x) => subscriptionKey(x) === selectedSub) ? selectedSub : null,
        revision: s.revision + 1,
      }));
      return;
    }
    const threads = useSettings.getState().threads && view.kind !== "search";
    const limit = Math.max(PAGE, messages.length);
    const page = await api.listMessages(view, 0, limit, threads);
    if (token !== listToken) return;
    const { selectedId } = get();
    let nextSelected = selectedId;
    if (selectedId != null && !page.items.some((m) => m.id === selectedId)) {
      // The selected row may now be represented by a newer message of the same thread.
      const old = messages.find((m) => m.id === selectedId);
      const sameThread = old && page.items.find((m) => m.threadKey === old.threadKey && m.accountId === old.accountId);
      nextSelected = sameThread ? sameThread.id : null;
    }
    set((s) => ({
      messages: page.items,
      hasMore: page.hasMore,
      loading: false,
      selectedId: nextSelected,
      revision: s.revision + 1,
    }));
  },

  refreshAll: async () => {
    await Promise.all([get().refreshAccounts(), get().refreshFolders(), get().refreshList()]);
  },

  setAccountStatus: (accountId, state, error) => {
    set({
      accounts: get().accounts.map((a) => (a.id === accountId ? { ...a, status: state, statusError: error } : a)),
    });
  },

  setView: (view) => {
    if (sameView(view, get().view)) return;
    set({
      view,
      lastView: view.kind === "search" ? get().lastView : view,
      messages: [],
      hasMore: false,
      serverHasOlder: true,
      loading: true,
      selectedId: null,
      selectedSub: null,
    });
    if (view.kind === "folder") void api.openFolder(view.folderId);
    // Views over one folder role refresh those folders in every account.
    if (view.kind === "unified" && (view.which === "sent" || view.which === "junk" || view.which === "trash")) {
      for (const f of get().folders) if (f.role === view.which) void api.openFolder(f.id);
    }
    void get().refreshList();
  },

  search: (query) => {
    const q = query.trim();
    if (!q) {
      if (get().view.kind === "search") get().setView(get().lastView);
      return;
    }
    get().setView({ kind: "search", query: q });
  },

  loadMore: async () => {
    const { view, messages, hasMore, loadingOlder, serverHasOlder } = get();
    if (view.kind === "drafts" || view.kind === "subscriptions" || loadingOlder) return;
    if (hasMore) {
      const threads = useSettings.getState().threads && view.kind !== "search";
      const page = await api.listMessages(view, messages.length, PAGE, threads);
      const known = new Set(get().messages.map((m) => m.id));
      set({ messages: [...get().messages, ...page.items.filter((m) => !known.has(m.id))], hasMore: page.hasMore });
      return;
    }
    if (view.kind === "folder" && serverHasOlder) {
      set({ loadingOlder: true });
      try {
        const more = await api.loadOlder(view.folderId);
        set({ serverHasOlder: more });
        await get().refreshList();
      } finally {
        set({ loadingOlder: false });
      }
    }
  },

  select: (selectedId) => set({ selectedId }),

  selectSub: (selectedSub) => set({ selectedSub }),

  selectRelative: (delta) => {
    const { messages, selectedId } = get();
    if (!messages.length) return;
    const i = messages.findIndex((m) => m.id === selectedId);
    const next = i < 0 ? 0 : Math.min(messages.length - 1, Math.max(0, i + delta));
    set({ selectedId: messages[next].id });
  },

  dropFromList: (ids) => {
    const { messages, selectedId } = get();
    const drop = new Set(ids);
    const remaining = messages.filter((m) => !drop.has(m.id));
    let nextSelected = selectedId;
    if (selectedId != null && drop.has(selectedId)) {
      const i = messages.findIndex((m) => m.id === selectedId);
      const after = messages.slice(i + 1).find((m) => !drop.has(m.id));
      const before = messages.slice(0, i).reverse().find((m) => !drop.has(m.id));
      nextSelected = (after ?? before)?.id ?? null;
    }
    set({ messages: remaining, selectedId: nextSelected });
  },
}));

export function accountById(id: string) {
  return useMail.getState().accounts.find((a) => a.id === id);
}
