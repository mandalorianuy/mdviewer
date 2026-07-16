import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

export interface OpenSelection {
  name: string;
  readToken: string;
  writeToken?: string;
}

export interface SaveSelection {
  name: string;
  writeToken: string;
}

export interface ConversionSource {
  name: string;
  readToken: string;
}

export interface OpenDocumentResult {
  content: string;
}

export interface SaveDocumentResult {
  saved: boolean;
  writeToken: string;
}

export interface ConversionRequest {
  operationId: string;
  sourceToken: string;
  outputToken: string;
}

export interface ConversionResult {
  operationId: string;
  markdownToken: string;
  warningCodes: string[];
  writeToken: string;
}

export interface ClaimedPrintJob {
  id: string;
  title: string;
  sourceToken: string;
  createdUnixMs: number;
}

export interface IntegrationStatus {
  deepLinkScheme?: string;
  printJobsAvailable?: boolean;
  networkAccess?: boolean;
  pendingPrintJobIds: string[];
}

export type MacosWorkflowStatus = "not_installed" | "installed" | "outdated" | "invalid";

export interface MacosIntegrationBackend {
  macosWorkflowStatus(): Promise<MacosWorkflowStatus>;
  installMacosWorkflow(): Promise<MacosWorkflowStatus>;
  repairMacosWorkflow(): Promise<MacosWorkflowStatus>;
  uninstallMacosWorkflow(): Promise<MacosWorkflowStatus>;
}

export interface CloseRequestEvent {
  preventDefault(): void;
}

export interface BackendError {
  code: string;
  message?: string;
}

export function isBackendErrorCode(reason: unknown, code: string): reason is BackendError {
  return typeof reason === "object"
    && reason !== null
    && "code" in reason
    && reason.code === code;
}

export interface Backend extends MacosIntegrationBackend {
  selectOpenDocument(): Promise<OpenSelection | null>;
  selectSaveDocument(suggestedName: string): Promise<SaveSelection | null>;
  selectConversionSource(): Promise<ConversionSource | null>;
  openDocument(token: string): Promise<OpenDocumentResult>;
  saveDocument(token: string, content: string): Promise<SaveDocumentResult>;
  convertDocument(request: ConversionRequest): Promise<ConversionResult>;
  cancelConversion(operationId: string): Promise<void>;
  claimPrintJob(id: string): Promise<ClaimedPrintJob>;
  finishPrintJob(id: string): Promise<void>;
  integrationStatus(): Promise<IntegrationStatus>;
  activateWindow(): Promise<void>;
  openExternal(url: string): Promise<void>;
  onPrintJobRequested(handler: (id: string) => void): Promise<UnlistenFn>;
  onCloseRequested(handler: (event: CloseRequestEvent) => void): Promise<UnlistenFn>;
}

interface WireOpenSelection {
  name: string;
  read_token: string;
  write_token?: string;
}

interface WireSaveSelection {
  name: string;
  write_token: string;
}

interface WireConversionResult {
  operation_id: string;
  markdown_token: string;
  warning_codes: string[];
  write_token: string;
}

interface WireSaveDocumentResult {
  saved: boolean;
  write_token: string;
}

interface WireClaimedPrintJob {
  id: string;
  title: string;
  source_token: string;
  created_unix_ms: number;
}

interface WireIntegrationStatus {
  deep_link_scheme: string;
  print_jobs_available: boolean;
  network_access: boolean;
  pending_print_job_ids: string[];
}

function openSelection(value: WireOpenSelection | null): OpenSelection | null {
  return value
    ? { name: value.name, readToken: value.read_token, writeToken: value.write_token }
    : null;
}

export const tauriBackend: Backend = {
  async selectOpenDocument() {
    return openSelection(await invoke<WireOpenSelection | null>("select_open_document"));
  },
  async selectSaveDocument(suggestedName) {
    const value = await invoke<WireSaveSelection | null>("select_save_document", {
      suggestedName,
    });
    return value ? { name: value.name, writeToken: value.write_token } : null;
  },
  async selectConversionSource() {
    const value = await invoke<WireOpenSelection | null>("select_conversion_source");
    const normalized = openSelection(value);
    return normalized ? { name: normalized.name, readToken: normalized.readToken } : null;
  },
  openDocument(token) {
    return invoke("open", { token });
  },
  saveDocument(token, content) {
    return invoke<WireSaveDocumentResult>("save", { token, content }).then((value) => ({
      saved: value.saved,
      writeToken: value.write_token,
    }));
  },
  async convertDocument(request) {
    const value = await invoke<WireConversionResult>("convert", {
      operationId: request.operationId,
      sourceToken: request.sourceToken,
      outputToken: request.outputToken,
    });
    return {
      operationId: value.operation_id,
      markdownToken: value.markdown_token,
      warningCodes: value.warning_codes,
      writeToken: value.write_token,
    };
  },
  cancelConversion(operationId) {
    return invoke("cancel", { operationId });
  },
  async claimPrintJob(id) {
    const value = await invoke<WireClaimedPrintJob>("claim_print_job", { id });
    return {
      id: value.id,
      title: value.title,
      sourceToken: value.source_token,
      createdUnixMs: value.created_unix_ms,
    };
  },
  finishPrintJob(id) {
    return invoke("finish_print_job", { id });
  },
  async integrationStatus() {
    const value = await invoke<WireIntegrationStatus>("integration_status");
    return {
      deepLinkScheme: value.deep_link_scheme,
      printJobsAvailable: value.print_jobs_available,
      networkAccess: value.network_access,
      pendingPrintJobIds: value.pending_print_job_ids,
    };
  },
  macosWorkflowStatus() {
    return invoke("macos_workflow_status");
  },
  installMacosWorkflow() {
    return invoke("install_macos_workflow");
  },
  repairMacosWorkflow() {
    return invoke("repair_macos_workflow");
  },
  uninstallMacosWorkflow() {
    return invoke("uninstall_macos_workflow");
  },
  async activateWindow() {
    const window = getCurrentWindow();
    await window.show();
    await window.setFocus();
  },
  openExternal(url) {
    return invoke("open_external", { url });
  },
  onPrintJobRequested(handler) {
    return listen<string>("print-job-requested", (event) => handler(event.payload));
  },
  async onCloseRequested(handler) {
    return getCurrentWindow().onCloseRequested(handler);
  },
};
