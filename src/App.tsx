import { useState, useEffect, useCallback } from "react";
import { Store } from "@tauri-apps/plugin-store";
import "./App.css";

const DEFAULT_WHISPER_PROMPT =
  "The following is a transcription of a dictation from a speaker who is XXX. The speaker sometimes discusses the following topics: YYY. The speaker sometimes uses the following uncommon terms: ZZZ.";
const DEFAULT_LLM_PROMPT =
  "You are a helpful assistant that will carefully examine the following transcription of a dictation and then carefully make the modifications requested of the editor.";

function App() {
  const [openAIKey, setOpenAIKey] = useState("");
  const [whisperPrompt, setWhisperPrompt] = useState("");
  const [llmPrompt, setLlmPrompt] = useState("");
  const [isSaved, setIsSaved] = useState(false);
  const [isLoading, setIsLoading] = useState(true);
  const [store, setStore] = useState<Store | null>(null);
  const [error, setError] = useState<string | null>(null);

  // Load settings from store
  const loadSettings = useCallback(async () => {
    try {
      setIsLoading(true);
      setError(null);

      console.log("Initializing store...");
      // Initialize store with options
      const storeInstance = await Store.load(".settings.dat", {
        createNew: false,
        autoSave: false, // We'll handle saving manually with debounce
      });
      console.log("Store loaded:", storeInstance);
      setStore(storeInstance);

      // Get values with default fallbacks
      const savedOpenAIKey =
        ((await storeInstance.get("openAIKey")) as string) || "";
      const savedWhisperPrompt =
        ((await storeInstance.get("whisperPrompt")) as string) || "";
      const savedLlmPrompt =
        ((await storeInstance.get("llmPrompt")) as string) || "";

      console.log("Loaded values:", {
        openAIKey: savedOpenAIKey ? "***" : "(empty)",
        whisperPrompt: savedWhisperPrompt ? "present" : "(empty)",
        llmPrompt: savedLlmPrompt ? "present" : "(empty)",
      });

      setOpenAIKey(savedOpenAIKey);
      setWhisperPrompt(savedWhisperPrompt || DEFAULT_WHISPER_PROMPT);
      setLlmPrompt(savedLlmPrompt || DEFAULT_LLM_PROMPT);
    } catch (error) {
      console.error("Failed to load settings:", error);
      setError(
        `Failed to load settings: ${
          error instanceof Error ? error.message : String(error)
        }`
      );
    } finally {
      setIsLoading(false);
    }
  }, []);

  // Save settings to store with debounce
  const saveSettings = useCallback(async () => {
    if (!store) {
      console.error("Cannot save settings: store not initialized");
      return;
    }

    try {
      console.log("Saving settings...");
      await store.set("openAIKey", openAIKey);
      await store.set("whisperPrompt", whisperPrompt);
      await store.set("llmPrompt", llmPrompt);
      await store.save();

      console.log("Settings saved successfully");
      setIsSaved(true);
      setTimeout(() => setIsSaved(false), 2000);
      setError(null);
    } catch (error) {
      console.error("Failed to save settings:", error);
      setError(
        `Failed to save settings: ${
          error instanceof Error ? error.message : String(error)
        }`
      );
    }
  }, [store, openAIKey, whisperPrompt, llmPrompt]);

  // Debounced save effect
  useEffect(() => {
    if (isLoading || !store) return; // Skip saving during initial load

    console.log("Changes detected, scheduling save...");
    const debounceTimer = setTimeout(() => {
      saveSettings();
    }, 1000); // 1 second debounce

    return () => clearTimeout(debounceTimer);
  }, [openAIKey, whisperPrompt, llmPrompt, saveSettings, isLoading, store]);

  // Load settings on mount
  useEffect(() => {
    loadSettings();
  }, [loadSettings]);

  return (
    <div className="container mx-auto p-4 max-w-3xl relative">
      {isSaved && (
        <div className="absolute top-2 right-2 badge badge-success gap-2 py-3 px-4">
          <svg
            xmlns="http://www.w3.org/2000/svg"
            fill="none"
            viewBox="0 0 24 24"
            className="inline-block w-4 h-4 stroke-current"
          >
            <path
              strokeLinecap="round"
              strokeLinejoin="round"
              strokeWidth="2"
              d="M5 13l4 4L19 7"
            ></path>
          </svg>
          Saved
        </div>
      )}

      {error && (
        <div className="alert alert-error mb-4">
          <svg
            xmlns="http://www.w3.org/2000/svg"
            className="stroke-current shrink-0 h-6 w-6"
            fill="none"
            viewBox="0 0 24 24"
          >
            <path
              strokeLinecap="round"
              strokeLinejoin="round"
              strokeWidth="2"
              d="M10 14l2-2m0 0l2-2m-2 2l-2-2m2 2l2 2m7-2a9 9 0 11-18 0 9 9 0 0118 0z"
            />
          </svg>
          <span>{error}</span>
        </div>
      )}

      <h1 className="text-2xl font-bold mb-6">Typr</h1>

      {/* Settings Section */}
      <h2 className="text-xl font-bold mb-4">Settings</h2>

      <div className="mb-6">
        Start dictating by pressing CMD+SHIFT+SPACE. Release to stop. The
        application will automatically transcribe your speech to text and then
        type wherever your cursor is. Experiment with adding custom vocabulary
        and instructions to customize the output to your liking.
      </div>

      {isLoading ? (
        <div className="flex justify-center items-center h-64">
          <span className="loading loading-spinner loading-lg"></span>
        </div>
      ) : (
        <form>
          <div className="form-control w-full mb-4 flex flex-col gap-2">
            <label className="label">
              <div className="label-text font-medium">OpenAI API Key</div>
              <div className="label-text-alt text-info text-xs">
                Your API key is stored locally and never shared
              </div>
            </label>
            <input
              type="password"
              placeholder="Enter your OpenAI API key"
              className="input input-bordered w-full"
              value={openAIKey}
              onChange={(e) => setOpenAIKey(e.target.value)}
            />
          </div>

          <div className="form-control w-full mb-4 flex flex-col gap-2">
            <label className="label flex items-baseline">
              <div className="label-text font-medium">Custom Vocabulary</div>
              <div className="label-text-alt text-info text-xs">
                This prompt will be given to the Speech-to-Text model. Providing
                relevant context and key words will result in a more accurate
                transcription.
              </div>
            </label>
            <textarea
              className="textarea textarea-bordered h-32"
              placeholder={DEFAULT_WHISPER_PROMPT}
              value={whisperPrompt}
              onChange={(e) => setWhisperPrompt(e.target.value)}
            ></textarea>
          </div>

          <div className="form-control w-full mb-6 flex flex-col gap-2">
            <label className="label">
              <div className="label-text font-medium">Custom Instructions</div>
              <div className="label-text-alt text-info text-xs">
                Add instructions to customize how your dictation is processed if
                you use the keywords "note to the editor" in the transcription.
              </div>
            </label>
            <textarea
              className="textarea textarea-bordered h-32"
              placeholder={DEFAULT_LLM_PROMPT}
              value={llmPrompt}
              onChange={(e) => setLlmPrompt(e.target.value)}
            ></textarea>
          </div>
        </form>
      )}
    </div>
  );
}

export default App;
