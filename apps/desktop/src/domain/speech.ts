/**
 * Multi-language Whisper domain contracts (Phase 12). Mirrors
 * `apps/desktop/src-tauri/src/commands.rs`'s `SpeechLanguageOptionDto`/
 * `SpeechLanguageCapabilitiesDto`. See `docs/phase-12-audit.md` for the
 * full design reasoning, including why Igbo is not among
 * `supportedLanguages`.
 */

export interface SpeechLanguageOption {
  code: string;
  name: string;
}

export interface SpeechLanguageCapabilities {
  currentLanguage: string;
  supportedLanguages: SpeechLanguageOption[];
  /** `null` until a model has actually loaded. */
  modelIsMultilingual: boolean | null;
}
