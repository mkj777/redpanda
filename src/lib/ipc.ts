// Typed wrappers around Tauri commands and events. In a plain browser (vite dev without
// Tauri) the calls go to an in-memory mock so the UI can be developed and tested standalone.

import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { listen as tauriListen, type UnlistenFn } from "@tauri-apps/api/event";

export type Security = "tls" | "start_tls" | "none";
export type AuthType = "password" | "app_password" | "oauth_google" | "oauth_microsoft";
export type SyncState = "idle" | "syncing" | "error" | "auth_required" | "offline";
export type FolderRole =
  | "inbox"
  | "sent"
  | "drafts"
  | "trash"
  | "junk"
  | "archive"
  | "all"
  | "flagged"
  | "important"
  | "other";

export interface ServerConfig {
  host: string;
  port: number;
  security: Security;
}

export interface Account {
  id: string;
  email: string;
  displayName: string;
  provider: string;
  authType: AuthType;
  username: string;
  imap: ServerConfig;
  smtp: ServerConfig;
  color: string;
  sentSavedByServer: boolean;
  isGmail: boolean;
  status: SyncState;
  statusError: string | null;
}

export interface Address {
  name: string | null;
  email: string;
}

export interface Folder {
  id: number;
  accountId: string;
  path: string;
  name: string;
  depth: number;
  role: FolderRole;
  unreadCount: number;
  totalCount: number;
}

export interface UnifiedCounts {
  inboxUnread: number;
  unread: number;
  starred: number;
  drafts: number;
}

export interface FolderList {
  folders: Folder[];
  unified: UnifiedCounts;
}

export interface MessageSummary {
  id: number;
  accountId: string;
  folderId: number;
  threadKey: string;
  from: Address | null;
  to: Address[];
  subject: string;
  date: number;
  seen: boolean;
  flagged: boolean;
  answered: boolean;
  hasAttachments: boolean;
  preview: string;
  threadCount: number;
}

export interface AttachmentInfo {
  index: number;
  filename: string;
  mimeType: string;
  size: number;
  inline: boolean;
}

export interface MessageDetail extends MessageSummary {
  cc: Address[];
  replyTo: Address[];
  messageId: string | null;
  references: string[];
  text: string | null;
  html: string | null;
  hasRemoteContent: boolean;
  remoteAllowed: boolean;
  attachments: AttachmentInfo[];
}

export interface MessagePage {
  items: MessageSummary[];
  hasMore: boolean;
}

export type UnifiedView = "inbox" | "unread" | "starred";

export type View =
  | { kind: "folder"; folderId: number }
  | { kind: "unified"; which: UnifiedView }
  | { kind: "search"; query: string }
  | { kind: "drafts" };

export interface Draft {
  id?: number | null;
  accountId: string;
  to: Address[];
  cc: Address[];
  bcc: Address[];
  subject: string;
  body: string;
  inReplyTo?: string | null;
  references: string[];
  attachments: string[];
  replyToMessageId?: number | null;
  forwardFromMessageId?: number | null;
  updatedAt?: number;
}

export interface DiscoveryResult {
  source: "provider" | "mx" | "autoconfig" | "srv" | "guess" | "none";
  providerId: string | null;
  providerName: string | null;
  authType: AuthType;
  allowPassword: boolean;
  imap: ServerConfig | null;
  smtp: ServerConfig | null;
  username: string;
  hint: string | null;
  sentSavedByServer: boolean;
  isGmail: boolean;
}

export type OAuthProvider = "google" | "microsoft";

export interface NewAccountInput {
  email: string;
  displayName: string;
  providerId: string | null;
  username: string;
  imap: ServerConfig;
  smtp: ServerConfig;
  auth: { type: "password"; password: string } | { type: "oauth"; provider: OAuthProvider };
}

export interface OAuthStatus {
  google: boolean;
  microsoft: boolean;
  configPath: string | null;
}

export interface AppError {
  code: string;
  message: string;
}

export type FlagKind = "seen" | "flagged";

export const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

type Args = Record<string, unknown>;

async function call<T>(cmd: string, args?: Args): Promise<T> {
  if (isTauri) return tauriInvoke<T>(cmd, args);
  const { mockInvoke } = await import("./mock");
  return mockInvoke(cmd, args ?? {}) as Promise<T>;
}

export function toAppError(e: unknown): AppError {
  if (e && typeof e === "object" && "code" in e && "message" in e) return e as AppError;
  return { code: "other", message: String(e) };
}

export const api = {
  listAccounts: () => call<Account[]>("list_accounts"),
  discover: (email: string) => call<DiscoveryResult>("discover", { email }),
  oauthStatus: () => call<OAuthStatus>("oauth_status"),
  addAccount: (input: NewAccountInput) => call<Account>("add_account", { input }),
  reauthAccount: (accountId: string, password?: string) =>
    call<void>("reauth_account", { accountId, password: password ?? null }),
  cancelOauth: () => call<void>("cancel_oauth"),
  updateAccount: (accountId: string, displayName: string, color: string) =>
    call<void>("update_account", { accountId, displayName, color }),
  removeAccount: (accountId: string) => call<void>("remove_account", { accountId }),
  listFolders: () => call<FolderList>("list_folders"),
  listMessages: (view: Exclude<View, { kind: "drafts" }>, offset: number, limit: number, threads: boolean) =>
    call<MessagePage>("list_messages", { view, offset, limit, threads }),
  openFolder: (folderId: number) => call<void>("open_folder", { folderId }),
  syncNow: (accountId?: string) => call<void>("sync_now", { accountId: accountId ?? null }),
  loadOlder: (folderId: number) => call<boolean>("load_older", { folderId }),
  getMessage: (id: number) => call<MessageDetail>("get_message", { id }),
  getThread: (id: number) => call<MessageSummary[]>("get_thread", { id }),
  setFlags: (ids: number[], flag: FlagKind, value: boolean, threads: boolean) =>
    call<void>("set_flags", { ids, flag, value, threads }),
  moveMessages: (ids: number[], targetFolderId: number, threads: boolean) =>
    call<void>("move_messages", { ids, targetFolderId, threads }),
  deleteMessages: (ids: number[], threads: boolean) => call<void>("delete_messages", { ids, threads }),
  archiveMessages: (ids: number[], threads: boolean) => call<void>("archive_messages", { ids, threads }),
  createArchiveFolder: (accountId: string) => call<void>("create_archive_folder", { accountId }),
  allowRemoteContent: (sender: string) => call<void>("allow_remote_content", { sender }),
  saveAttachment: (messageId: number, index: number, path: string) =>
    call<void>("save_attachment", { messageId, index, path }),
  openAttachment: (messageId: number, index: number) => call<void>("open_attachment", { messageId, index }),
  sendMessage: (draft: Draft) => call<void>("send_message", { draft }),
  saveDraft: (draft: Draft) => call<number>("save_draft", { draft }),
  listDrafts: () => call<Draft[]>("list_drafts"),
  getDraft: (id: number) => call<Draft>("get_draft", { id }),
  deleteDraft: (id: number) => call<void>("delete_draft", { id }),
  suggestContacts: (prefix: string) => call<Address[]>("suggest_contacts", { prefix }),
};

export interface MailChanged {
  accountId: string;
  folderId: number | null;
}

export interface SyncStatusEvent {
  accountId: string;
  state: SyncState;
  error: string | null;
}

export interface ActionFailed {
  accountId: string;
  error: AppError;
}

type Events = {
  "mail:changed": MailChanged;
  "sync:status": SyncStatusEvent;
  "action:failed": ActionFailed;
};

export async function listen<K extends keyof Events>(
  event: K,
  handler: (payload: Events[K]) => void,
): Promise<UnlistenFn> {
  if (isTauri) return tauriListen<Events[K]>(event, (e) => handler(e.payload));
  const { mockListen } = await import("./mock");
  return mockListen(event, handler as (p: unknown) => void);
}

export async function openExternal(url: string) {
  if (isTauri) {
    const { openUrl } = await import("@tauri-apps/plugin-opener");
    await openUrl(url);
  } else {
    window.open(url, "_blank", "noopener");
  }
}

export async function pickFiles(): Promise<string[]> {
  if (!isTauri) return [];
  const { open } = await import("@tauri-apps/plugin-dialog");
  const result = await open({ multiple: true, directory: false });
  if (!result) return [];
  return Array.isArray(result) ? result : [result];
}

export async function pickSavePath(defaultName: string): Promise<string | null> {
  if (!isTauri) return null;
  const { save } = await import("@tauri-apps/plugin-dialog");
  return save({ defaultPath: defaultName });
}
