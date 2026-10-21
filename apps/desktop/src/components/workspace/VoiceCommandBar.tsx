import { useEffect, useRef, useState } from "react";
import type { UnifiedItemAction } from "./actions";
import { ACTION_LABELS } from "./actions";
import { parseOperatorVoiceCommand, type OperatorVoiceCommand } from "../../lib/operatorVoiceCommands";

interface SpeechRecognitionLike {
  continuous: boolean;
  interimResults: boolean;
  lang: string;
  start(): void;
  stop(): void;
  onresult: ((event: { results: { [index: number]: { [index: number]: { transcript: string } }; length: number } }) => void) | null;
  onerror: (() => void) | null;
  onend: (() => void) | null;
}

interface VoiceCommandBarProps {
  availableActions: UnifiedItemAction[];
  onCommand: (command: OperatorVoiceCommand) => void;
}

export function VoiceCommandBar({ availableActions, onCommand }: VoiceCommandBarProps) {
  const [text, setText] = useState("");
  const [armed, setArmed] = useState<OperatorVoiceCommand | null>(null);
  const [listening, setListening] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const recognitionRef = useRef<SpeechRecognitionLike | null>(null);

  useEffect(() => () => recognitionRef.current?.stop(), []);

  const listen = () => {
    const speechWindow = window as Window & {
      SpeechRecognition?: new () => SpeechRecognitionLike;
      webkitSpeechRecognition?: new () => SpeechRecognitionLike;
    };
    const Recognition = speechWindow.SpeechRecognition ?? speechWindow.webkitSpeechRecognition;
    if (!Recognition) {
      setMessage("Speech recognition is unavailable here. Type or paste the transcript instead.");
      return;
    }
    const recognition = new Recognition();
    recognition.continuous = false;
    recognition.interimResults = false;
    recognition.lang = "en-US";
    recognition.onresult = (event) => setText(event.results[0][0].transcript);
    recognition.onerror = () => setMessage("Could not hear a command. Try again or type it below.");
    recognition.onend = () => setListening(false);
    recognitionRef.current = recognition;
    setMessage(null);
    setListening(true);
    recognition.start();
  };

  const interpret = () => {
    const command = parseOperatorVoiceCommand(text, availableActions);
    if (!command) {
      setArmed(null);
      setMessage("Command not recognized. Try “display the next suggestion” or “open diagnostics mode”.");
      return;
    }
    setArmed(command);
    setMessage("Review the command, then confirm it before CIP acts.");
  };

  const confirm = () => {
    if (!armed) return;
    onCommand(armed);
    setArmed(null);
    setMessage("Command sent to the existing operator action guard.");
  };

  return (
    <div className="voice-command-bar" aria-label="Operator voice commands">
      <div className="voice-command-bar__header">
        <strong>Voice command</strong>
        <span>operator confirmation required</span>
      </div>
      <div className="voice-command-bar__controls">
        <input
          value={text}
          onChange={(event) => setText(event.target.value)}
          placeholder="e.g. display the next suggestion"
          aria-label="Voice command transcript"
        />
        <button type="button" onClick={listen} disabled={listening}>
          {listening ? "Listening…" : "Listen"}
        </button>
        <button type="button" onClick={interpret} disabled={!text.trim()}>
          Interpret
        </button>
      </div>
      {armed && (
        <div className="voice-command-bar__confirmation" role="status">
          <span>
            {armed.kind === "top-item-action" ? ACTION_LABELS[armed.action] : armed.kind === "open-diagnostics" ? "Open diagnostics" : "Focus search"}
            {armed.kind === "top-item-action" && " on the first attention item"}
          </span>
          <button type="button" onClick={confirm}>Confirm</button>
          <button type="button" onClick={() => setArmed(null)}>Cancel</button>
        </div>
      )}
      {message && <small>{message}</small>}
    </div>
  );
}
