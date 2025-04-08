import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Store } from "@tauri-apps/plugin-store";
import "./App.css";

function App() {
  const [openAIKey, setOpenAIKey] = useState("");
  const [customVocabulary, setCustomVocabulary] = useState("");
  const [customInstructions, setCustomInstructions] = useState("");
  const [isSaved, setIsSaved] = useState(false);
  const [isLoading, setIsLoading] = useState(true);
  const [store, setStore] = useState<Store | null>(null);
  const [error, setError] = useState<string | null>(null);

  // Audio recording state
  const [isRecording, setIsRecording] = useState(false);
  const [transcription, setTranscription] = useState("");
  const [openaiResponse, setOpenaiResponse] = useState("");
  const [isProcessing, setIsProcessing] = useState(false);
  const [audioDevices, setAudioDevices] = useState<
    Array<{ name: string; id: string }>
  >([]);
  const [isLoadingDevices, setIsLoadingDevices] = useState(false);

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
      const savedCustomVocabulary =
        ((await storeInstance.get("customVocabulary")) as string) || "";
      const savedCustomInstructions =
        ((await storeInstance.get("customInstructions")) as string) || "";

      console.log("Loaded values:", {
        openAIKey: savedOpenAIKey ? "***" : "(empty)",
        vocabulary: savedCustomVocabulary ? "present" : "(empty)",
        instructions: savedCustomInstructions ? "present" : "(empty)",
      });

      setOpenAIKey(savedOpenAIKey);
      setCustomVocabulary(savedCustomVocabulary);
      setCustomInstructions(savedCustomInstructions);
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
      await store.set("customVocabulary", customVocabulary);
      await store.set("customInstructions", customInstructions);
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
  }, [store, openAIKey, customVocabulary, customInstructions]);

  // Debounced save effect
  useEffect(() => {
    if (isLoading || !store) return; // Skip saving during initial load

    console.log("Changes detected, scheduling save...");
    const debounceTimer = setTimeout(() => {
      saveSettings();
    }, 1000); // 1 second debounce

    return () => clearTimeout(debounceTimer);
  }, [
    openAIKey,
    customVocabulary,
    customInstructions,
    saveSettings,
    isLoading,
    store,
  ]);

  // Load settings on mount
  useEffect(() => {
    loadSettings();
  }, [loadSettings]);

  // Check recording status periodically
  useEffect(() => {
    const checkRecordingStatus = async () => {
      try {
        const status = await invoke<boolean>("is_recording");
        setIsRecording(status);
      } catch (error) {
        console.error("Failed to check recording status:", error);
      }
    };

    const interval = setInterval(checkRecordingStatus, 1000);
    return () => clearInterval(interval);
  }, []);

  // Handle start recording
  const handleStartRecording = async () => {
    try {
      await invoke("start_recording");
      setIsRecording(true);
      setError(null);
    } catch (error) {
      console.error("Failed to start recording:", error);
      setError(
        `Failed to start recording: ${
          error instanceof Error ? error.message : String(error)
        }`
      );
    }
  };

  // Handle stop recording and process
  const handleStopRecording = async () => {
    try {
      setIsProcessing(true);
      setError(null);

      const result = await invoke<{
        transcription: string;
        openai_response: string;
      }>("stop_recording_and_process");

      setTranscription(result.transcription);
      setOpenaiResponse(result.openai_response);
      setIsRecording(false);
    } catch (error) {
      console.error("Failed to stop recording:", error);
      setError(
        `Failed to stop recording: ${
          error instanceof Error ? error.message : String(error)
        }`
      );
    } finally {
      setIsProcessing(false);
    }
  };

  // Handle listing audio input devices
  const handleListAudioDevices = async () => {
    try {
      setIsLoadingDevices(true);
      setError(null);

      const devices = await invoke<Array<{ name: string; id: string }>>(
        "get_audio_input_devices"
      );
      setAudioDevices(devices);
    } catch (error) {
      console.error("Failed to list audio devices:", error);
      setError(
        `Failed to list audio devices: ${
          error instanceof Error ? error.message : String(error)
        }`
      );
    } finally {
      setIsLoadingDevices(false);
    }
  };

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

      <h1 className="text-2xl font-bold mb-6">Dictation App</h1>

      {/* Audio Recording Controls */}
      <div className="card bg-base-200 shadow-xl mb-6">
        <div className="card-body">
          <h2 className="card-title">Audio Recording</h2>
          <p>
            Use the global shortcut (Cmd+Shift+Space) or the buttons below to
            record audio.
          </p>

          <div className="flex gap-4 mt-4">
            <button
              className={`btn ${isRecording ? "btn-error" : "btn-primary"}`}
              onClick={isRecording ? handleStopRecording : handleStartRecording}
              disabled={isProcessing}
            >
              {isRecording ? "Stop Recording" : "Start Recording"}
              {isRecording && (
                <span className="loading loading-spinner loading-xs ml-2"></span>
              )}
            </button>

            <button
              className="btn btn-outline"
              onClick={handleListAudioDevices}
              disabled={isLoadingDevices}
            >
              {isLoadingDevices ? (
                <span className="loading loading-spinner loading-xs mr-2"></span>
              ) : (
                "List Audio Devices"
              )}
            </button>

            {isProcessing && (
              <div className="flex items-center">
                <span className="loading loading-spinner loading-md mr-2"></span>
                <span>Processing audio...</span>
              </div>
            )}
          </div>

          {audioDevices.length > 0 && (
            <div className="mt-4">
              <h3 className="font-medium">Available Audio Input Devices:</h3>
              <div className="overflow-x-auto mt-2">
                <table className="table table-zebra w-full">
                  <thead>
                    <tr>
                      <th>ID</th>
                      <th>Name</th>
                    </tr>
                  </thead>
                  <tbody>
                    {audioDevices.map((device) => (
                      <tr key={device.id}>
                        <td>{device.id}</td>
                        <td>{device.name}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </div>
          )}

          {transcription && (
            <div className="mt-4">
              <h3 className="font-medium">Transcription:</h3>
              <div className="p-4 bg-base-300 rounded-lg mt-2">
                {transcription}
              </div>
            </div>
          )}

          {openaiResponse && (
            <div className="mt-4">
              <h3 className="font-medium">OpenAI Response:</h3>
              <div className="p-4 bg-base-300 rounded-lg mt-2">
                {openaiResponse}
              </div>
            </div>
          )}
        </div>
      </div>

      {/* Settings Section */}
      <h2 className="text-xl font-bold mb-4">Settings</h2>

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
                Add specialized terms or phrases to improve transcription
                accuracy
              </div>
            </label>
            <textarea
              className="textarea textarea-bordered h-32"
              placeholder="Enter specialized terms, one per line"
              value={customVocabulary}
              onChange={(e) => setCustomVocabulary(e.target.value)}
            ></textarea>
          </div>

          <div className="form-control w-full mb-6 flex flex-col gap-2">
            <label className="label">
              <div className="label-text font-medium">Custom Instructions</div>
              <div className="label-text-alt text-info text-xs">
                Add instructions to customize how your dictation is processed
              </div>
            </label>
            <textarea
              className="textarea textarea-bordered h-32"
              placeholder="Enter custom instructions for transcription"
              value={customInstructions}
              onChange={(e) => setCustomInstructions(e.target.value)}
            ></textarea>
          </div>
        </form>
      )}
    </div>
  );
}

export default App;
