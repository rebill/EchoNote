export type EchoNoteErrorCode =
  | "UNSUPPORTED_PLATFORM"
  | "MIC_PERMISSION_DENIED"
  | "ASR_SERVICE_START_FAILED"
  | "ASR_SERVICE_UNAVAILABLE"
  | "ASR_MODEL_LOAD_FAILED"
  | "ASR_TRANSCRIBE_FAILED"
  | "ASR_FINALIZE_FAILED"
  | "ASR_COMPANION_UNAVAILABLE"
  | "ASR_COMPANION_DISCOVERY_INVALID"
  | "ASR_COMPANION_DISCOVERY_STALE"
  | "MEETING_STOP_FAILED"
  | "NOTE_CREATE_FAILED"
  | "NOTE_WRITE_FAILED"
  | "LLM_CONFIG_MISSING"
  | "LLM_REQUEST_FAILED"
  | "LLM_RESPONSE_PARSE_FAILED";

export type EchoNoteError = {
  code: EchoNoteErrorCode;
  message: string;
  detail?: string;
  recoverable: boolean;
  createdAt: number;
};

export function createEchoNoteError(
  code: EchoNoteErrorCode,
  message: string,
  options: { detail?: string; recoverable?: boolean } = {}
): EchoNoteError {
  return {
    code,
    message,
    detail: options.detail,
    recoverable: options.recoverable ?? true,
    createdAt: Date.now()
  };
}
