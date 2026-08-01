import AppKit
import ApplicationServices
import AVFoundation
import CoreGraphics
import MoonshineVoice
import ServiceManagement
import SwiftUI

private let runsURL = FileManager.default.homeDirectoryForCurrentUser
    .appendingPathComponent("Library/Application Support/Typr/Runs", isDirectory: true)
private let moonshineModelURL = FileManager.default.homeDirectoryForCurrentUser
    .appendingPathComponent(
        "Library/Caches/moonshine_voice/download.moonshine.ai/model/medium-streaming-en/quantized",
        isDirectory: true
    )
private let profileURL = FileManager.default.homeDirectoryForCurrentUser
    .appendingPathComponent("Library/Application Support/Typr/Profile/profile.json")
private let profileRevisionsURL = FileManager.default.homeDirectoryForCurrentUser
    .appendingPathComponent("Library/Application Support/Typr/Profile/Revisions", isDirectory: true)

private struct AppContext: Codable {
    let applicationName: String?
    let bundleIdentifier: String?
    let windowTitle: String?
    let document: String?
    let role: String?
    let label: String?
    let textBeforeCursor: String?
    let selectedText: String?
    let textAfterCursor: String?
    let visibleText: String?
    let textContextStatus: String?
}

private enum InsertionVerification {
    case inserted
    case missing
    case unavailable
}

private enum Activity {
    case idle
    case recording
    case feedbackRecording
    case processing
}

private struct CaptureDiagnostics: Encodable {
    let backend: String
    let sampleRate: Double
    let channels: Int
    let format: String
    let bufferCount: Int
    let capturedFrames: Int64
    let discontinuityCount: Int
    let missingFrames: Int64
    let overlappingFrames: Int64
    let writeError: String?
}

private struct MoonshineStreamingResult {
    let transcript: String?
    let finalizeMs: Int
    let error: String?
}

/*
 * The model stays loaded for the app's lifetime. Each dictation gets a fresh
 * stream so encoder and decoder caches never leak across utterances.
 */
private final class MoonshineStreamingSession {
    private let queue: DispatchQueue
    private let stream: MoonshineVoice.Stream
    private var lines: [UInt64: (startTime: Float, text: String)] = [:]
    private var error: String?

    init(transcriber: Transcriber, queue: DispatchQueue) throws {
        self.queue = queue
        stream = try queue.sync {
            try transcriber.createStream(updateInterval: 0.5)
        }
        try queue.sync {
            stream.addListener { [weak self] event in
                guard let self else {
                    return
                }
                if let failure = event as? TranscriptError {
                    error = failure.error.localizedDescription
                    return
                }
                let text = event.line.text.trimmingCharacters(in: .whitespacesAndNewlines)
                if !text.isEmpty {
                    lines[event.line.lineId] = (event.line.startTime, text)
                }
            }
            try stream.start()
        }
    }

    func addAudio(_ audio: [Float], sampleRate: Int32) {
        queue.async { [weak self] in
            guard let self else {
                return
            }
            do {
                try stream.addAudio(audio, sampleRate: sampleRate)
            } catch {
                self.error = error.localizedDescription
            }
        }
    }

    func stop() -> MoonshineStreamingResult {
        queue.sync {
            let startedAt = ProcessInfo.processInfo.systemUptime
            do {
                try stream.stop()
            } catch {
                self.error = error.localizedDescription
            }
            stream.close()
            let transcript = lines.values
                .sorted { $0.startTime < $1.startTime }
                .map(\.text)
                .joined(separator: " ")
                .trimmingCharacters(in: .whitespacesAndNewlines)
            return MoonshineStreamingResult(
                transcript: transcript.isEmpty ? nil : transcript,
                finalizeMs: Int(
                    (ProcessInfo.processInfo.systemUptime - startedAt) * 1_000
                ),
                error: self.error
            )
        }
    }
}

/*
 * FFmpeg's AVFoundation input can drop audio frames on macOS. Recording from
 * Core Audio's native input node avoids that adapter and preserves the exact
 * hardware buffers for diagnosis before any conversion or normalization.
 */
private final class NativeRecorder {
    let sourceURL: URL
    private let engine = AVAudioEngine()
    private let format: AVAudioFormat
    private var audioFile: AVAudioFile?
    private var expectedSampleTime: AVAudioFramePosition?
    private var bufferCount = 0
    private var capturedFrames: AVAudioFramePosition = 0
    private var discontinuityCount = 0
    private var missingFrames: AVAudioFramePosition = 0
    private var overlappingFrames: AVAudioFramePosition = 0
    private var writeError: String?

    init(
        sourceURL: URL,
        onAudio: @escaping ([Float], Int32) -> Void
    ) throws {
        self.sourceURL = sourceURL
        format = engine.inputNode.outputFormat(forBus: 0)
        guard format.sampleRate > 0, format.channelCount > 0 else {
            throw NSError(
                domain: "Typr",
                code: 1,
                userInfo: [NSLocalizedDescriptionKey: "No microphone input is available"]
            )
        }

        audioFile = try AVAudioFile(forWriting: sourceURL, settings: format.settings)
        engine.inputNode.installTap(
            onBus: 0,
            bufferSize: 4_096,
            format: format
        ) { [weak self] buffer, time in
            guard let self else {
                return
            }
            if time.isSampleTimeValid {
                if let expectedSampleTime, time.sampleTime != expectedSampleTime {
                    let difference = time.sampleTime - expectedSampleTime
                    discontinuityCount += 1
                    if difference > 0 {
                        missingFrames += difference
                    } else {
                        overlappingFrames += -difference
                    }
                }
                expectedSampleTime = time.sampleTime + AVAudioFramePosition(buffer.frameLength)
            }
            do {
                try audioFile?.write(from: buffer)
            } catch {
                writeError = error.localizedDescription
            }
            bufferCount += 1
            capturedFrames += AVAudioFramePosition(buffer.frameLength)
            if let channel = buffer.floatChannelData?[0] {
                onAudio(
                    Array(UnsafeBufferPointer(
                        start: channel,
                        count: Int(buffer.frameLength)
                    )),
                    Int32(format.sampleRate)
                )
            }
        }
        engine.prepare()
        do {
            try engine.start()
        } catch {
            engine.inputNode.removeTap(onBus: 0)
            audioFile = nil
            throw error
        }
    }

    func stop(diagnosticsURL: URL) -> Int {
        engine.inputNode.removeTap(onBus: 0)
        engine.stop()
        audioFile = nil
        let diagnostics = CaptureDiagnostics(
            backend: "AVAudioEngine",
            sampleRate: format.sampleRate,
            channels: Int(format.channelCount),
            format: String(describing: format.commonFormat),
            bufferCount: bufferCount,
            capturedFrames: capturedFrames,
            discontinuityCount: discontinuityCount,
            missingFrames: missingFrames,
            overlappingFrames: overlappingFrames,
            writeError: writeError
        )
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        if let data = try? encoder.encode(diagnostics) {
            try? data.write(to: diagnosticsURL, options: .atomic)
        }
        return Int(Double(capturedFrames) / format.sampleRate * 1_000)
    }
}

private struct TextSnapshot {
    let value: String?
    let selection: CFRange?
}

/*
 * Accessibility support varies substantially between native controls,
 * browsers, editors, and terminals. Every read is optional so one unsupported
 * attribute never prevents dictation.
 */
private enum Accessibility {
    static func focusedElement() -> AXUIElement? {
        copyAttribute(
            AXUIElementCreateSystemWide(),
            kAXFocusedUIElementAttribute
        )
    }

    static func captureContext() -> AppContext {
        guard let element = focusedElement() else {
            return AppContext(
                applicationName: NSWorkspace.shared.frontmostApplication?.localizedName,
                bundleIdentifier: NSWorkspace.shared.frontmostApplication?.bundleIdentifier,
                windowTitle: nil,
                document: nil,
                role: nil,
                label: nil,
                textBeforeCursor: nil,
                selectedText: nil,
                textAfterCursor: nil,
                visibleText: nil,
                textContextStatus: "No focused Accessibility element"
            )
        }

        var pid: pid_t = 0
        AXUIElementGetPid(element, &pid)
        let runningApplication = NSRunningApplication(processIdentifier: pid)
        let bundleIdentifier = runningApplication?.bundleIdentifier
        let application = AXUIElementCreateApplication(pid)
        let window: AXUIElement? = copyAttribute(application, kAXFocusedWindowAttribute)
        let value: String? = copyAttribute(element, kAXValueAttribute)
        let selection = rangeAttribute(element, kAXSelectedTextRangeAttribute)

        let usesTerminalTextContext = [
            "com.mitchellh.ghostty",
            "com.apple.Terminal",
            "com.googlecode.iterm2",
            "com.github.wez.wezterm",
            "org.alacritty",
            "net.kovidgoyal.kitty",
            "dev.warp.Warp-Stable"
        ].contains { $0 == bundleIdentifier }
        let nearbyText: (before: String?, selected: String?, after: String?) = {
            guard !usesTerminalTextContext else {
                return (nil, nil, nil)
            }
            guard let value, let selection else {
                return (nil, copyAttribute(element, kAXSelectedTextAttribute), nil)
            }
            let text = value as NSString
            guard selection.location >= 0,
                  selection.length >= 0,
                  selection.location + selection.length <= text.length else {
                return (nil, copyAttribute(element, kAXSelectedTextAttribute), nil)
            }
            return (
                String(text.substring(to: selection.location).suffix(1_000)),
                text.substring(with: NSRange(
                    location: selection.location,
                    length: selection.length
                )),
                String(text.substring(from: selection.location + selection.length).prefix(1_000))
            )
        }()

        let visibleText: String? = {
            if usesTerminalTextContext {
                // Terminal Accessibility caret ranges commonly remain at zero
                // while tmux redraws. The buffer tail reflects the active pane.
                return value.map { String($0.suffix(4_000)) }
            }
            guard nearbyText.before == nil, nearbyText.after == nil else {
                return nil
            }
            if let visibleRange = rangeAttribute(element, kAXVisibleCharacterRangeAttribute),
               let text = string(element, for: visibleRange) {
                return String(text.suffix(2_000))
            }
            return value.map { String($0.suffix(2_000)) }
        }()

        return AppContext(
            applicationName: runningApplication?.localizedName,
            bundleIdentifier: bundleIdentifier,
            windowTitle: window.flatMap { copyAttribute($0, kAXTitleAttribute) },
            document: copyStringOrURL(element, kAXDocumentAttribute)
                ?? window.flatMap { copyStringOrURL($0, kAXDocumentAttribute) },
            role: copyAttribute(element, kAXRoleAttribute),
            label: copyAttribute(element, kAXTitleAttribute)
                ?? copyAttribute(element, kAXDescriptionAttribute)
                ?? copyAttribute(element, kAXPlaceholderValueAttribute),
            textBeforeCursor: nearbyText.before,
            selectedText: nearbyText.selected,
            textAfterCursor: nearbyText.after,
            visibleText: visibleText,
            textContextStatus: usesTerminalTextContext
                ? "Terminal context captured from the current screen-buffer tail; unreliable caret ignored"
                : selection == nil
                ? "Focused control did not expose a selected-text range"
                : "Text captured around the Accessibility caret"
        )
    }

    static func insert(_ text: String) -> String {
        guard let element = focusedElement() else {
            typeWithUnicodeEvents(text)
            return "typing"
        }

        let beforeAccessibility = snapshot(element)
        var isSettable = DarwinBoolean(false)
        if AXUIElementIsAttributeSettable(
            element,
            kAXSelectedTextAttribute as CFString,
            &isSettable
        ) == .success,
           isSettable.boolValue,
           AXUIElementSetAttributeValue(
               element,
               kAXSelectedTextAttribute as CFString,
               text as CFString
           ) == .success {
            Thread.sleep(forTimeInterval: 0.03)
            switch verifyInsertion(
                text,
                before: beforeAccessibility,
                after: snapshot(element)
            ) {
            case .inserted, .unavailable:
                return "accessibility"
            case .missing:
                break
            }
        }

        let beforePaste = snapshot(element)
        let pasteboard = NSPasteboard.general
        let previousPasteboardItems = pasteboard.pasteboardItems?.map { item in
            let copy = NSPasteboardItem()
            item.types.forEach { type in
                if let data = item.data(forType: type) {
                    copy.setData(data, forType: type)
                }
            }
            return copy
        }
        pasteboard.clearContents()
        pasteboard.setString(text, forType: .string)
        let typrPasteboardChangeCount = pasteboard.changeCount
        defer {
            // A copy made while insertion is in flight owns the clipboard; do
            // not replace it with the value that predated this dictation.
            if pasteboard.changeCount == typrPasteboardChangeCount {
                pasteboard.clearContents()
                if let previousPasteboardItems, !previousPasteboardItems.isEmpty {
                    pasteboard.writeObjects(previousPasteboardItems)
                }
            }
        }
        if pressPasteMenuItem(for: element) {
            // Invoking the command itself respects terminal-specific shortcuts
            // without synthesizing a key combination the user may have remapped.
            Thread.sleep(forTimeInterval: 0.15)
            switch verifyInsertion(text, before: beforePaste, after: snapshot(element)) {
            case .inserted, .unavailable:
                return "paste"
            case .missing:
                break
            }
        }
        postPaste()
        Thread.sleep(forTimeInterval: 0.15)
        switch verifyInsertion(text, before: beforePaste, after: snapshot(element)) {
        case .inserted, .unavailable:
            return "paste"
        case .missing:
            typeWithUnicodeEvents(text)
            return "typing"
        }
    }

    private static func pressPasteMenuItem(for element: AXUIElement) -> Bool {
        var pid: pid_t = 0
        guard AXUIElementGetPid(element, &pid) == .success,
              let menuBar: AXUIElement = copyAttribute(
                  AXUIElementCreateApplication(pid),
                  kAXMenuBarAttribute
              ) else {
            return false
        }

        let pressPasteInDescendants: ([AXUIElement]) -> Bool = { roots in
            /*
             * Menu hierarchies vary across AppKit, Electron, and native
             * terminals, so search descendants rather than assuming a depth.
             */
            var pending = roots
            var visited = 0
            while !pending.isEmpty && visited < 500 {
                let candidate = pending.removeFirst()
                visited += 1
                let title: String? = copyAttribute(candidate, kAXTitleAttribute)
                let role: String? = copyAttribute(candidate, kAXRoleAttribute)
                if role == kAXMenuItemRole,
                   title?.trimmingCharacters(in: .whitespacesAndNewlines)
                       .localizedCaseInsensitiveCompare("Paste") == .orderedSame,
                   AXUIElementPerformAction(candidate, kAXPressAction as CFString) == .success {
                    return true
                }
                let children: [AXUIElement]? = copyAttribute(candidate, kAXChildrenAttribute)
                pending.append(contentsOf: children ?? [])
            }
            return false
        }
        if pressPasteInDescendants([menuBar]) {
            return true
        }

        // Some apps materialize menu children only while their parent is open.
        let topLevelItems: [AXUIElement]? = copyAttribute(menuBar, kAXChildrenAttribute)
        guard let editMenu = topLevelItems?.first(where: { item in
            let title: String? = copyAttribute(item, kAXTitleAttribute)
            return title?.localizedCaseInsensitiveCompare("Edit") == .orderedSame
        }), AXUIElementPerformAction(editMenu, kAXPressAction as CFString) == .success else {
            return false
        }
        Thread.sleep(forTimeInterval: 0.02)
        let didPaste = pressPasteInDescendants([editMenu])
        if !didPaste {
            AXUIElementPerformAction(editMenu, kAXCancelAction as CFString)
        }
        return didPaste
    }

    private static func copyAttribute<T>(
        _ element: AXUIElement,
        _ attribute: String
    ) -> T? {
        var value: CFTypeRef?
        guard AXUIElementCopyAttributeValue(
            element,
            attribute as CFString,
            &value
        ) == .success else {
            return nil
        }
        return value as? T
    }

    private static func copyStringOrURL(
        _ element: AXUIElement,
        _ attribute: String
    ) -> String? {
        if let value: String = copyAttribute(element, attribute) {
            return value
        }
        if let value: URL = copyAttribute(element, attribute) {
            return value.absoluteString
        }
        return nil
    }

    private static func rangeAttribute(
        _ element: AXUIElement,
        _ attribute: String
    ) -> CFRange? {
        var value: CFTypeRef?
        guard AXUIElementCopyAttributeValue(
            element,
            attribute as CFString,
            &value
        ) == .success,
              let value,
              CFGetTypeID(value) == AXValueGetTypeID() else {
            return nil
        }
        var range = CFRange()
        guard AXValueGetValue(value as! AXValue, .cfRange, &range) else {
            return nil
        }
        return range
    }

    private static func string(
        _ element: AXUIElement,
        for range: CFRange
    ) -> String? {
        var range = range
        guard let value = AXValueCreate(.cfRange, &range) else {
            return nil
        }
        var result: CFTypeRef?
        guard AXUIElementCopyParameterizedAttributeValue(
            element,
            kAXStringForRangeParameterizedAttribute as CFString,
            value,
            &result
        ) == .success else {
            return nil
        }
        return result as? String
    }

    private static func snapshot(_ element: AXUIElement) -> TextSnapshot {
        TextSnapshot(
            value: copyAttribute(element, kAXValueAttribute),
            selection: rangeAttribute(element, kAXSelectedTextRangeAttribute)
        )
    }

    /*
     * A successful AX write is trusted when the target does not expose text
     * for verification. Falling through in that case risks inserting twice.
     */
    private static func verifyInsertion(
        _ insertedText: String,
        before: TextSnapshot,
        after: TextSnapshot
    ) -> InsertionVerification {
        guard let beforeValue = before.value,
              let afterValue = after.value else {
            return .unavailable
        }
        guard let selection = before.selection else {
            if beforeValue == afterValue {
                return .missing
            }
            return afterValue.contains(insertedText) ? .inserted : .missing
        }

        let original = beforeValue as NSString
        guard selection.location >= 0,
              selection.length >= 0,
              selection.location + selection.length <= original.length else {
            return .unavailable
        }
        let expected = original.replacingCharacters(
            in: NSRange(location: selection.location, length: selection.length),
            with: insertedText
        )
        return afterValue == expected ? .inserted : .missing
    }

    private static func postPaste() {
        guard let source = CGEventSource(stateID: .combinedSessionState),
              let keyDown = CGEvent(
                  keyboardEventSource: source,
                  virtualKey: 9,
                  keyDown: true
              ),
              let keyUp = CGEvent(
                  keyboardEventSource: source,
                  virtualKey: 9,
                  keyDown: false
              ) else {
            return
        }
        keyDown.flags = .maskCommand
        keyUp.flags = .maskCommand
        keyDown.post(tap: .cghidEventTap)
        keyUp.post(tap: .cghidEventTap)
    }

    private static func typeWithUnicodeEvents(_ text: String) {
        text.reduce(into: [String]()) { chunks, character in
            if chunks.isEmpty ||
                chunks[chunks.count - 1].utf16.count + character.utf16.count > 8 {
                chunks.append(String(character))
            } else {
                chunks[chunks.count - 1].append(character)
            }
        }.forEach { chunk in
            guard let source = CGEventSource(stateID: .combinedSessionState),
                  let keyDown = CGEvent(
                      keyboardEventSource: source,
                      virtualKey: 0,
                      keyDown: true
                  ),
                  let keyUp = CGEvent(
                      keyboardEventSource: source,
                      virtualKey: 0,
                      keyDown: false
                  ) else {
                return
            }
            let characters = Array(chunk.utf16)
            characters.withUnsafeBufferPointer { buffer in
                guard let address = buffer.baseAddress else {
                    return
                }
                keyDown.keyboardSetUnicodeString(
                    stringLength: characters.count,
                    unicodeString: address
                )
                keyUp.keyboardSetUnicodeString(
                    stringLength: characters.count,
                    unicodeString: address
                )
            }
            keyDown.post(tap: .cghidEventTap)
            keyUp.post(tap: .cghidEventTap)
            // TUIs can discard bursts of synthetic text even when macOS accepts every event.
            Thread.sleep(forTimeInterval: 0.008)
        }
    }
}

private struct ModelTiming: Decodable {
    let totalMs: Int
    let loadMs: Int
    let promptEvalMs: Int
    let generationMs: Int
    let promptTokens: Int
    let outputTokens: Int
    let tokensPerSecond: Double
}

private struct RunMetadata: Decodable {
    let createdAt: String
    let status: String
    let kind: String?
    let recordingMs: Int?
    let transcriptionMs: Int?
    let rewriteMs: Int?
    let rewriteTiming: ModelTiming?
    let moonshineTranscriptionMs: Int?
    let moonshineRewriteMs: Int?
    let moonshineRewriteTiming: ModelTiming?
    let whisperKitTranscriptionMs: Int?
    let whisperKitRewriteMs: Int?
    let whisperKitRewriteTiming: ModelTiming?
    let hotPathMs: Int?
    let evaluationMs: Int?
    let evaluationError: String?
    let totalProcessingMs: Int?
    let transcriptionModel: String?
    let referenceTranscriptionModel: String?
    let rewriteModel: String?
    let insertionMethod: String?
    let error: String?
}

private struct ArchivedRewriteRequest: Decodable {
    let model: String
    let system: String
    let prompt: String
}

private struct AudioVariant: Identifiable, Hashable {
    let id: String
    let label: String
    let detail: String
    let url: URL
}

private struct ProfileRule: Codable, Identifiable {
    let id: String
    let text: String
}

private struct ProfileDocument: Codable {
    let version: Int
    let rules: [ProfileRule]
}

@MainActor
private final class ProfileStore: ObservableObject {
    @Published var rules: [ProfileRule] = []
    @Published var error: String?
    @Published var revisionCount = 0
    @Published var status: String?

    init() {
        reload()
    }

    func reload() {
        do {
            let data = try Data(contentsOf: profileURL)
            rules = try JSONDecoder().decode(ProfileDocument.self, from: data).rules
            revisionCount = ((try? FileManager.default.contentsOfDirectory(
                at: profileRevisionsURL,
                includingPropertiesForKeys: nil
            )) ?? []).filter { $0.pathExtension == "json" }.count
            error = nil
        } catch {
            rules = []
            self.error = error.localizedDescription
        }
    }

    func undoLastUpdate() {
        guard revisionCount > 0,
              let executable = Bundle.main.url(forResource: "typr", withExtension: nil) else {
            return
        }
        status = "Undoing…"
        let process = Process()
        process.executableURL = executable
        process.arguments = ["profile-undo"]
        var environment = ProcessInfo.processInfo.environment
        environment["HOME"] = FileManager.default.homeDirectoryForCurrentUser.path
        process.environment = environment
        process.terminationHandler = { [weak self] process in
            DispatchQueue.main.async {
                self?.status = process.terminationStatus == 0 ? "Update undone" : "Undo failed"
                self?.reload()
            }
        }
        do {
            try process.run()
        } catch {
            status = "Undo failed"
        }
    }
}

private struct ProfileView: View {
    @StateObject private var store = ProfileStore()

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack {
                VStack(alignment: .leading, spacing: 3) {
                    Text("Justin’s profile")
                        .font(.title2.bold())
                    Text("Hold Shift+Fn to teach Typr. Each update is versioned locally.")
                        .foregroundStyle(.secondary)
                }
                Spacer()
                Button {
                    store.reload()
                } label: {
                    Image(systemName: "arrow.clockwise")
                }
                Button("Undo Last Update") {
                    store.undoLastUpdate()
                }
                .disabled(store.revisionCount == 0)
            }
            .padding(20)
            Divider()
            if let status = store.status {
                Text(status)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .padding(.horizontal, 20)
                    .padding(.top, 10)
            }
            if store.rules.isEmpty {
                ContentUnavailableView(
                    "No profile rules",
                    systemImage: "person.text.rectangle",
                    description: Text(store.error ?? "Use Shift+Fn to add the first rule.")
                )
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                List(store.rules) { rule in
                    Text(rule.text)
                        .textSelection(.enabled)
                        .padding(.vertical, 5)
                }
            }
        }
        .frame(minWidth: 680, minHeight: 520)
    }
}

private struct ModelTimingView: View {
    let timing: ModelTiming

    var body: some View {
        HStack(spacing: 12) {
            Text("generation \(timing.generationMs)ms")
                .fontWeight(.semibold)
            Text("\(timing.outputTokens) tokens · \(timing.tokensPerSecond, specifier: "%.1f") tok/s")
            Text("prompt \(timing.promptEvalMs)ms")
            Text("load \(timing.loadMs)ms")
            Text("total \(timing.totalMs)ms")
        }
        .font(.caption)
        .foregroundStyle(.secondary)
    }
}

private struct DeveloperOverlayData {
    let runID: String
    let rawTranscript: String
    let output: String
    let source: String
    let transcriptionMs: Int?
    let hotPathMs: Int?
    let timing: ModelTiming?
    let referenceSource: String?
    let referenceTranscript: String?
    let referenceOutput: String?
    let referenceTiming: ModelTiming?
    let context: AppContext?
    let contextJSON: String?
    let captureJSON: String?

    var diagnostics: String {
        [
            "Request ID: \(runID)",
            "Typed source: \(source)",
            hotPathMs.map { "Stop to insertion: \($0)ms" },
            transcriptionMs.map { "Transcription: \($0)ms" },
            timing.map {
                "Qwen: generation \($0.generationMs)ms, prompt \($0.promptEvalMs)ms, load \($0.loadMs)ms, total \($0.totalMs)ms, \($0.outputTokens) tokens at \($0.tokensPerSecond) tok/s"
            },
            "\nRaw transcript:\n\(rawTranscript)",
            "\nInserted output:\n\(output)",
            contextJSON.map { "\nAccessibility context:\n\($0)" },
            captureJSON.map { "\nCapture diagnostics:\n\($0)" }
        ].compactMap { $0 }.joined(separator: "\n")
    }
}

private struct DeveloperOverlayView: View {
    let data: DeveloperOverlayData
    let close: () -> Void
    let openDetails: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack(alignment: .top) {
                VStack(alignment: .leading, spacing: 3) {
                    Text("Typr Developer")
                        .font(.title2.bold())
                    Text([
                        data.hotPathMs.map { "typed \($0)ms" },
                        data.transcriptionMs.map { "\(data.source) \($0)ms" },
                        data.timing.map { "Qwen generation \($0.generationMs)ms" }
                    ].compactMap { $0 }.joined(separator: " · "))
                        .foregroundStyle(.secondary)
                }
                Spacer()
                Button {
                    close()
                } label: {
                    Image(systemName: "xmark.circle.fill")
                        .font(.title2)
                        .foregroundStyle(.secondary)
                }
                .buttonStyle(.plain)
            }

            if let timing = data.timing {
                ModelTimingView(timing: timing)
            }

            ScrollView {
                VStack(alignment: .leading, spacing: 14) {
                    HStack(alignment: .top, spacing: 12) {
                        GroupBox("Original dictation") {
                            Text(data.rawTranscript)
                                .frame(maxWidth: .infinity, alignment: .leading)
                                .textSelection(.enabled)
                                .padding(4)
                        }
                        .frame(maxWidth: .infinity, alignment: .top)
                        GroupBox("Inserted text") {
                            Text(data.output)
                                .frame(maxWidth: .infinity, alignment: .leading)
                                .textSelection(.enabled)
                                .padding(4)
                        }
                        .frame(maxWidth: .infinity, alignment: .top)
                    }

                    GroupBox("Accessibility context") {
                        VStack(alignment: .leading, spacing: 7) {
                            ForEach([
                                ("Application", data.context?.applicationName),
                                ("Bundle", data.context?.bundleIdentifier),
                                ("Window", data.context?.windowTitle),
                                ("Document", data.context?.document),
                                ("Role", data.context?.role),
                                ("Label", data.context?.label),
                                ("Before cursor", data.context?.textBeforeCursor),
                                ("Selection", data.context?.selectedText),
                                ("After cursor", data.context?.textAfterCursor),
                                ("Visible text", data.context?.visibleText),
                                ("Text context status", data.context?.textContextStatus)
                            ].compactMap { label, value in
                                value.map { (label, $0) }
                            }, id: \.0) { label, value in
                                VStack(alignment: .leading, spacing: 2) {
                                    Text(label)
                                        .font(.caption.bold())
                                        .foregroundStyle(.secondary)
                                    Text(value)
                                        .textSelection(.enabled)
                                }
                            }
                            if data.context == nil {
                                Text("No Accessibility context was captured.")
                                    .foregroundStyle(.secondary)
                            }
                        }
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(4)
                    }

                    if let referenceSource = data.referenceSource,
                       let referenceTranscript = data.referenceTranscript,
                       let referenceOutput = data.referenceOutput {
                        GroupBox("Reference path · \(referenceSource)") {
                            VStack(alignment: .leading, spacing: 8) {
                                Text("Raw: \(referenceTranscript)")
                                    .textSelection(.enabled)
                                Text("Output: \(referenceOutput)")
                                    .textSelection(.enabled)
                                if let timing = data.referenceTiming {
                                    ModelTimingView(timing: timing)
                                }
                            }
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .padding(4)
                        }
                    }
                }
            }

            HStack {
                Button("Copy Request ID") {
                    NSPasteboard.general.clearContents()
                    NSPasteboard.general.setString(data.runID, forType: .string)
                }
                Button("Copy Output") {
                    NSPasteboard.general.clearContents()
                    NSPasteboard.general.setString(data.output, forType: .string)
                }
                Button("Copy Diagnostics") {
                    NSPasteboard.general.clearContents()
                    NSPasteboard.general.setString(data.diagnostics, forType: .string)
                }
                Spacer()
                Button("Open Full Details") {
                    openDetails()
                }
            }
        }
        .padding(18)
        .frame(minWidth: 720, minHeight: 560)
        .background(.regularMaterial)
    }
}

private struct DictationRun: Identifiable, Hashable {
    let id: String
    let directory: URL
    let metadata: RunMetadata
    let transcript: String?
    let output: String?
    let moonshineTranscript: String?
    let moonshineOutput: String?
    let whisperKitTranscript: String?
    let whisperKitOutput: String?
    let request: ArchivedRewriteRequest?
    let context: String?
    let capture: String?
    let profileUpdate: String?
    let alternatives: [(name: String, text: String)]
    let audioVariants: [AudioVariant]

    static func == (lhs: DictationRun, rhs: DictationRun) -> Bool {
        lhs.id == rhs.id
    }

    func hash(into hasher: inout Hasher) {
        hasher.combine(id)
    }
}

@MainActor
private final class HistoryStore: NSObject, ObservableObject, AVAudioPlayerDelegate {
    @Published var runs: [DictationRun] = []
    @Published var selectedID: String?
    @Published var rewriteModel = "qwen3.5:4b"
    @Published var rewriteStatus: String?
    @Published var playingAudioID: String?
    private var audioPlayer: AVAudioPlayer?

    override init() {
        super.init()
        reload()
    }

    func reload() {
        let urls = (try? FileManager.default.contentsOfDirectory(
            at: runsURL,
            includingPropertiesForKeys: nil,
            options: [.skipsHiddenFiles]
        )) ?? []
        runs = urls.compactMap { directory -> DictationRun? in
            guard let data = try? Data(contentsOf: directory.appendingPathComponent("metadata.json")),
                  let metadata = try? JSONDecoder().decode(RunMetadata.self, from: data) else {
                return nil
            }
            let alternativesURL = directory.appendingPathComponent("alternatives", isDirectory: true)
            let alternatives = ((try? FileManager.default.contentsOfDirectory(
                at: alternativesURL,
                includingPropertiesForKeys: nil,
                options: [.skipsHiddenFiles]
            )) ?? []).filter {
                $0.pathExtension == "txt"
                    && !$0.deletingPathExtension().lastPathComponent.hasSuffix(".response")
                    && !$0.deletingPathExtension().lastPathComponent.hasSuffix(".fallback")
            }.sorted { $0.lastPathComponent > $1.lastPathComponent }.compactMap { url -> (name: String, text: String)? in
                guard let text = try? String(contentsOf: url, encoding: .utf8) else {
                    return nil
                }
                return (url.deletingPathExtension().lastPathComponent, text)
            }
            let audioVariants = [
                AudioVariant(
                    id: "\(directory.lastPathComponent)-native",
                    label: "Native",
                    detail: "Core Audio · lossless",
                    url: directory.appendingPathComponent("audio-native.caf")
                ),
                AudioVariant(
                    id: "\(directory.lastPathComponent)-aac",
                    label: "AAC",
                    detail: "M4A · 128 kbps",
                    url: directory.appendingPathComponent("audio-aac.m4a")
                ),
                AudioVariant(
                    id: "\(directory.lastPathComponent)-original",
                    label: "FFmpeg original",
                    detail: "Legacy · 48 kHz",
                    url: directory.appendingPathComponent("audio-original.wav")
                ),
                AudioVariant(
                    id: "\(directory.lastPathComponent)-normalized",
                    label: "Normalized",
                    detail: "48 kHz · 24-bit",
                    url: directory.appendingPathComponent("audio-normalized.wav")
                ),
                AudioVariant(
                    id: "\(directory.lastPathComponent)-asr",
                    label: "ASR",
                    detail: "16 kHz · 16-bit",
                    url: directory.appendingPathComponent("audio-asr.wav")
                )
            ].filter { FileManager.default.fileExists(atPath: $0.url.path) }
            let legacyAudioURL = directory.appendingPathComponent("audio.wav")
            return DictationRun(
                id: directory.lastPathComponent,
                directory: directory,
                metadata: metadata,
                transcript: try? String(
                    contentsOf: directory.appendingPathComponent("transcript.txt"),
                    encoding: .utf8
                ),
                output: try? String(
                    contentsOf: directory.appendingPathComponent("output.txt"),
                    encoding: .utf8
                ),
                moonshineTranscript: try? String(
                    contentsOf: directory.appendingPathComponent("transcript-moonshine.txt"),
                    encoding: .utf8
                ),
                moonshineOutput: try? String(
                    contentsOf: directory.appendingPathComponent("output-moonshine.txt"),
                    encoding: .utf8
                ),
                whisperKitTranscript: try? String(
                    contentsOf: directory.appendingPathComponent("transcript-whisperkit.txt"),
                    encoding: .utf8
                ),
                whisperKitOutput: try? String(
                    contentsOf: directory.appendingPathComponent("output-whisperkit.txt"),
                    encoding: .utf8
                ),
                request: try? JSONDecoder().decode(
                    ArchivedRewriteRequest.self,
                    from: Data(contentsOf: directory.appendingPathComponent("request.json"))
                ),
                context: try? String(
                    contentsOf: directory.appendingPathComponent("context.json"),
                    encoding: .utf8
                ),
                capture: try? String(
                    contentsOf: directory.appendingPathComponent("capture.json"),
                    encoding: .utf8
                ),
                profileUpdate: try? String(
                    contentsOf: directory.appendingPathComponent("profile-update.json"),
                    encoding: .utf8
                ),
                alternatives: alternatives,
                audioVariants: audioVariants.isEmpty && FileManager.default.fileExists(atPath: legacyAudioURL.path)
                    ? [AudioVariant(
                        id: "\(directory.lastPathComponent)-legacy",
                        label: "Recording",
                        detail: "Legacy",
                        url: legacyAudioURL
                    )]
                    : audioVariants
            )
        }.sorted { $0.metadata.createdAt > $1.metadata.createdAt }
        if selectedID == nil || !runs.contains(where: { $0.id == selectedID }) {
            selectedID = runs.first?.id
        }
    }

    func togglePlayback(_ variant: AudioVariant) {
        if playingAudioID == variant.id {
            audioPlayer?.stop()
            playingAudioID = nil
            return
        }
        do {
            audioPlayer = try AVAudioPlayer(contentsOf: variant.url)
            audioPlayer?.delegate = self
            audioPlayer?.play()
            playingAudioID = variant.id
        } catch {
            playingAudioID = nil
        }
    }

    func rewrite(_ run: DictationRun) {
        guard !rewriteModel.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
              let executable = Bundle.main.url(forResource: "typr", withExtension: nil) else {
            return
        }
        rewriteStatus = "Rewriting…"
        let process = Process()
        process.executableURL = executable
        process.arguments = ["rewrite", run.id, "--model", rewriteModel]
        var environment = ProcessInfo.processInfo.environment
        environment["HOME"] = FileManager.default.homeDirectoryForCurrentUser.path
        environment["PATH"] =
            "/opt/homebrew/bin:/opt/homebrew/sbin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"
        process.environment = environment
        process.terminationHandler = { [weak self] process in
            DispatchQueue.main.async {
                self?.rewriteStatus = process.terminationStatus == 0 ? "Saved alternate" : "Rewrite failed"
                self?.reload()
            }
        }
        do {
            try process.run()
        } catch {
            rewriteStatus = "Rewrite failed"
        }
    }

    nonisolated func audioPlayerDidFinishPlaying(
        _ player: AVAudioPlayer,
        successfully flag: Bool
    ) {
        Task { @MainActor [weak self] in
            self?.playingAudioID = nil
        }
    }
}

private struct HistoryView: View {
    @StateObject private var store = HistoryStore()

    var body: some View {
        NavigationSplitView {
            List(store.runs, selection: $store.selectedID) { run in
                VStack(alignment: .leading, spacing: 3) {
                    Text(run.metadata.createdAt)
                        .font(.headline)
                        .lineLimit(1)
                    Text(run.metadata.kind == "profile-feedback"
                        ? "profile feedback · \(run.metadata.status)"
                        : run.metadata.status)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                .tag(run.id)
            }
            .navigationTitle("Dictations")
            .toolbar {
                Button {
                    store.reload()
                } label: {
                    Image(systemName: "arrow.clockwise")
                }
            }
        } detail: {
            if let run = store.runs.first(where: { $0.id == store.selectedID }) {
                ScrollView {
                    VStack(alignment: .leading, spacing: 16) {
                        HStack {
                            Button("Copy Request ID") {
                                NSPasteboard.general.clearContents()
                                NSPasteboard.general.setString(run.id, forType: .string)
                            }
                            ForEach(run.audioVariants) { variant in
                                Button {
                                    store.togglePlayback(variant)
                                } label: {
                                    VStack(alignment: .leading, spacing: 1) {
                                        Text(store.playingAudioID == variant.id ? "Stop" : variant.label)
                                        Text(variant.detail)
                                            .font(.caption2)
                                            .foregroundStyle(.secondary)
                                    }
                                }
                            }
                            Button("Reveal in Finder") {
                                NSWorkspace.shared.activateFileViewerSelecting(
                                    run.audioVariants.first.map { [$0.url] } ?? [run.directory]
                                )
                            }
                            Spacer()
                            Text([
                                run.metadata.hotPathMs.map { "typed \($0)ms" },
                                run.metadata.moonshineTranscriptionMs.map { "Moonshine \($0)ms" },
                                run.metadata.whisperKitTranscriptionMs.map { "WhisperKit \($0)ms" },
                                run.metadata.rewriteTiming.map { "Qwen generation \($0.generationMs)ms" },
                                run.metadata.insertionMethod.map { "insert \($0)" }
                            ].compactMap { $0 }.joined(separator: " · "))
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }

                        if let moonshineTranscript = run.moonshineTranscript,
                           let moonshineOutput = run.moonshineOutput,
                           let whisperKitTranscript = run.whisperKitTranscript,
                           let whisperKitOutput = run.whisperKitOutput {
                            HStack(alignment: .top, spacing: 16) {
                                VStack(alignment: .leading, spacing: 12) {
                                    GroupBox(
                                        run.metadata.transcriptionModel == "medium-streaming-en"
                                            ? "Moonshine raw · typed path"
                                            : "Moonshine raw"
                                    ) {
                                        Text(moonshineTranscript)
                                            .frame(maxWidth: .infinity, alignment: .leading)
                                            .textSelection(.enabled)
                                            .padding(4)
                                    }
                                    GroupBox("Moonshine rewritten") {
                                        Text(moonshineOutput)
                                            .frame(maxWidth: .infinity, alignment: .leading)
                                            .textSelection(.enabled)
                                            .padding(4)
                                    }
                                    if let timing = run.metadata.moonshineRewriteTiming {
                                        ModelTimingView(timing: timing)
                                    }
                                }
                                .frame(maxWidth: .infinity, alignment: .leading)
                                VStack(alignment: .leading, spacing: 12) {
                                    GroupBox(
                                        run.metadata.transcriptionModel == "medium-streaming-en"
                                            ? "WhisperKit raw"
                                            : "WhisperKit raw · typed path"
                                    ) {
                                        Text(whisperKitTranscript)
                                            .frame(maxWidth: .infinity, alignment: .leading)
                                            .textSelection(.enabled)
                                            .padding(4)
                                    }
                                    GroupBox("WhisperKit rewritten") {
                                        Text(whisperKitOutput)
                                            .frame(maxWidth: .infinity, alignment: .leading)
                                            .textSelection(.enabled)
                                            .padding(4)
                                    }
                                    if let timing = run.metadata.whisperKitRewriteTiming {
                                        ModelTimingView(timing: timing)
                                    }
                                }
                                .frame(maxWidth: .infinity, alignment: .leading)
                            }
                        } else {
                            GroupBox("Raw transcript") {
                                Text(run.transcript ?? "No transcript")
                                    .frame(maxWidth: .infinity, alignment: .leading)
                                    .textSelection(.enabled)
                                    .padding(4)
                            }
                            GroupBox("Cleaned output") {
                                Text(run.output ?? run.metadata.error ?? "No output")
                                    .frame(maxWidth: .infinity, alignment: .leading)
                                    .textSelection(.enabled)
                                    .padding(4)
                            }
                            if let timing = run.metadata.rewriteTiming {
                                ModelTimingView(timing: timing)
                            }
                        }

                        if let evaluationError = run.metadata.evaluationError {
                            Text("Reference path unavailable: \(evaluationError)")
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }

                        if let capture = run.capture {
                            DisclosureGroup("Capture diagnostics") {
                                Text(capture)
                                    .font(.system(.body, design: .monospaced))
                                    .frame(maxWidth: .infinity, alignment: .leading)
                                    .textSelection(.enabled)
                                    .padding(.top, 8)
                            }
                        }

                        if let profileUpdate = run.profileUpdate {
                            DisclosureGroup("Profile operations") {
                                Text(profileUpdate)
                                    .font(.system(.body, design: .monospaced))
                                    .frame(maxWidth: .infinity, alignment: .leading)
                                    .textSelection(.enabled)
                                    .padding(.top, 8)
                            }
                        }

                        if let request = run.request {
                            DisclosureGroup("Exact model request") {
                                VStack(alignment: .leading, spacing: 12) {
                                    Text("Model: \(request.model)")
                                        .font(.caption)
                                        .foregroundStyle(.secondary)
                                    GroupBox("System prompt") {
                                        Text(request.system)
                                            .frame(maxWidth: .infinity, alignment: .leading)
                                            .textSelection(.enabled)
                                            .padding(4)
                                    }
                                    GroupBox("User prompt and page context") {
                                        Text(request.prompt)
                                            .frame(maxWidth: .infinity, alignment: .leading)
                                            .textSelection(.enabled)
                                            .padding(4)
                                    }
                                    if let context = run.context {
                                        GroupBox("Raw Accessibility context") {
                                            Text(context)
                                                .font(.system(.body, design: .monospaced))
                                                .frame(maxWidth: .infinity, alignment: .leading)
                                                .textSelection(.enabled)
                                                .padding(4)
                                        }
                                    }
                                }
                                .padding(.top, 8)
                            }
                        }

                        HStack {
                            TextField("Ollama model", text: $store.rewriteModel)
                                .textFieldStyle(.roundedBorder)
                            Button("Run alternate") {
                                store.rewrite(run)
                            }
                            if let status = store.rewriteStatus {
                                Text(status)
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                            }
                        }

                        ForEach(Array(run.alternatives.enumerated()), id: \.offset) { _, alternate in
                            GroupBox(alternate.name) {
                                Text(alternate.text)
                                    .frame(maxWidth: .infinity, alignment: .leading)
                                    .textSelection(.enabled)
                                    .padding(4)
                            }
                        }
                    }
                    .padding(20)
                }
                .navigationTitle("Run details")
            } else {
                VStack(spacing: 12) {
                    Image(systemName: "waveform")
                        .font(.largeTitle)
                    Text("No Dictations")
                        .font(.title2)
                    Text("Hold Fn to record your first dictation.")
                        .foregroundStyle(.secondary)
                }
            }
        }
        .frame(minWidth: 900, minHeight: 600)
    }
}

private final class AppDelegate: NSObject, NSApplicationDelegate, NSMenuDelegate {
    private let statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.squareLength)
    private let statusMenuItem = NSMenuItem(title: "Starting…", action: nil, keyEquivalent: "")
    private let launchAtLoginMenuItem = NSMenuItem(
        title: "Start at Login",
        action: #selector(toggleLaunchAtLogin),
        keyEquivalent: ""
    )
    private let completionNotificationsMenuItem = NSMenuItem(
        title: "Completion Notifications",
        action: #selector(toggleCompletionNotifications),
        keyEquivalent: ""
    )
    private let developerOverlayMenuItem = NSMenuItem(
        title: "Developer Overlay",
        action: #selector(toggleDeveloperOverlay),
        keyEquivalent: ""
    )
    private let hotkeyTestMenuItem = NSMenuItem(
        title: "Test Shift+Fn…",
        action: #selector(beginHotkeyTest),
        keyEquivalent: ""
    )
    private let recentDictationsSeparator = NSMenuItem.separator()
    private var recentDictationMenuItems: [NSMenuItem] = []
    private var eventTap: CFMachPort?
    private var isFnPressed = false
    private var isShiftPressed = false
    private var isHotkeyTestActive = false
    private var hotkeyTestTimer: Timer?
    private var isRecording = false
    private var nativeRecorder: NativeRecorder?
    private let moonshineQueue = DispatchQueue(
        label: "com.jcarrus.typr.moonshine",
        qos: .userInitiated
    )
    private var moonshineTranscriber: Transcriber?
    private var moonshineSession: MoonshineStreamingSession?
    private var currentRunID: String?
    private var currentContextURL: URL?
    private var currentContextCapture: DispatchWorkItem?
    private var currentCreatedAt: String?
    private var currentIsProfileFeedback = false
    private var activeDictations: [ObjectIdentifier: (process: Process, activity: Activity)] = [:]
    private var historyWindow: NSWindow?
    private var profileWindow: NSWindow?
    private var developerOverlayPanel: NSPanel?
    private var developerOverlayWatchTimer: Timer?

    func applicationDidFinishLaunching(_ notification: Notification) {
        ProcessInfo.processInfo.disableAutomaticTermination(
            "Typr listens for Fn while running in the menu bar"
        )
        NSApp.setActivationPolicy(.accessory)
        statusItem.length = 30
        statusItem.button?.image = NSImage(
            systemSymbolName: "waveform",
            accessibilityDescription: "Typr"
        )
        updateActivity(.idle)

        let menu = NSMenu()
        menu.delegate = self
        statusMenuItem.isEnabled = false
        menu.addItem(statusMenuItem)
        menu.addItem(recentDictationsSeparator)
        menu.addItem(
            withTitle: "Dictation History…",
            action: #selector(openHistory),
            keyEquivalent: ""
        ).target = self
        menu.addItem(
            withTitle: "Profile…",
            action: #selector(openProfile),
            keyEquivalent: ""
        ).target = self
        completionNotificationsMenuItem.target = self
        completionNotificationsMenuItem.state = UserDefaults.standard.object(
            forKey: "showCompletionNotifications"
        ) == nil || UserDefaults.standard.bool(forKey: "showCompletionNotifications") ? .on : .off
        menu.addItem(completionNotificationsMenuItem)
        developerOverlayMenuItem.target = self
        developerOverlayMenuItem.state = UserDefaults.standard.bool(
            forKey: "showDeveloperOverlay"
        ) ? .on : .off
        menu.addItem(developerOverlayMenuItem)
        hotkeyTestMenuItem.target = self
        menu.addItem(hotkeyTestMenuItem)
        menu.addItem(.separator())
        launchAtLoginMenuItem.target = self
        launchAtLoginMenuItem.state = SMAppService.mainApp.status == .enabled ? .on : .off
        menu.addItem(launchAtLoginMenuItem)
        menu.addItem(
            withTitle: "Open Accessibility Settings…",
            action: #selector(openAccessibilitySettings),
            keyEquivalent: ""
        ).target = self
        menu.addItem(.separator())
        menu.addItem(
            withTitle: "Quit Typr",
            action: #selector(quit),
            keyEquivalent: "q"
        ).target = self
        statusItem.menu = menu

        refreshRecentDictations()
        loadMoonshine()
        requestAccessibility()
    }

    private func loadMoonshine() {
        moonshineQueue.async { [weak self] in
            do {
                let transcriber = try Transcriber(
                    modelPath: moonshineModelURL.path,
                    modelArch: .mediumStreaming
                )
                DispatchQueue.main.async {
                    self?.moonshineTranscriber = transcriber
                }
            } catch {
                // WhisperKit remains available when Moonshine cannot load.
            }
        }
    }

    /*
     * Accessibility grants both event listening and event posting. Using the
     * app bundle as the permission identity keeps approval stable and visible
     * in System Settings, unlike a temporary command-line helper.
     */
    private func requestAccessibility() {
        let options = [
            kAXTrustedCheckOptionPrompt.takeUnretainedValue() as String: true
        ] as CFDictionary
        guard AXIsProcessTrustedWithOptions(options) else {
            openAccessibilitySettings()
            NSApp.terminate(nil)
            return
        }
        startListening()
        _ = runEngine("warmup")
    }

    /*
     * Fn is a modifier rather than a normal shortcut, so macOS's global-hotkey
     * APIs cannot register it alone. flagsChanged exposes both edges needed for
     * hold-to-record without consuming Fn or changing its behavior elsewhere.
     */
    private func startListening() {
        guard eventTap == nil else {
            return
        }

        isFnPressed = CGEventSource.flagsState(.combinedSessionState)
            .contains(.maskSecondaryFn)
        isShiftPressed = CGEventSource.flagsState(.combinedSessionState)
            .contains(.maskShift)
        eventTap = CGEvent.tapCreate(
            tap: .cgSessionEventTap,
            place: .headInsertEventTap,
            options: .listenOnly,
            eventsOfInterest: CGEventMask(1) << CGEventType.flagsChanged.rawValue,
            callback: { _, type, event, userInfo in
                guard let userInfo else {
                    return Unmanaged.passUnretained(event)
                }
                let app = Unmanaged<AppDelegate>
                    .fromOpaque(userInfo)
                    .takeUnretainedValue()

                if type == .tapDisabledByTimeout || type == .tapDisabledByUserInput {
                    if let eventTap = app.eventTap {
                        CGEvent.tapEnable(tap: eventTap, enable: true)
                    }
                    return Unmanaged.passUnretained(event)
                }

                let isFnPressed = event.flags.contains(.maskSecondaryFn)
                let isShiftPressed = event.flags.contains(.maskShift)
                let didFnChange = isFnPressed != app.isFnPressed
                let didShiftChange = isShiftPressed != app.isShiftPressed
                let didPressShift = isShiftPressed && !app.isShiftPressed
                guard didFnChange || didShiftChange else {
                    return Unmanaged.passUnretained(event)
                }
                app.isFnPressed = isFnPressed
                app.isShiftPressed = isShiftPressed
                DispatchQueue.main.async {
                    if app.isHotkeyTestActive {
                        app.updateHotkeyTest(
                            isFnPressed: isFnPressed,
                            isShiftPressed: isShiftPressed,
                            didFnChange: didFnChange
                        )
                    } else if didFnChange {
                        app.setRecording(
                            isEnabled: isFnPressed,
                            isProfileFeedback: isFnPressed && isShiftPressed
                        )
                    } else if isFnPressed && didPressShift {
                        app.promoteRecordingToProfileFeedback()
                    }
                }
                return Unmanaged.passUnretained(event)
            },
            userInfo: Unmanaged.passUnretained(self).toOpaque()
        )

        guard let eventTap else {
            statusMenuItem.title = "Unable to monitor Fn"
            return
        }
        let source = CFMachPortCreateRunLoopSource(kCFAllocatorDefault, eventTap, 0)
        CFRunLoopAddSource(CFRunLoopGetMain(), source, .commonModes)
        CGEvent.tapEnable(tap: eventTap, enable: true)
        statusMenuItem.title = "Hold Fn to dictate"
    }

    /*
     * Feedback mode latches for the recording once both modifiers overlap.
     * This makes Shift→Fn and Fn→Shift equivalent instead of depending on the
     * modifier order exposed by a particular keyboard or remapping utility.
     */
    private func promoteRecordingToProfileFeedback() {
        guard isRecording, !currentIsProfileFeedback else {
            return
        }
        currentIsProfileFeedback = true
        updateActivity(.feedbackRecording)
    }

    private func updateHotkeyTest(
        isFnPressed: Bool,
        isShiftPressed: Bool,
        didFnChange: Bool
    ) {
        if isFnPressed && isShiftPressed {
            isHotkeyTestActive = false
            hotkeyTestTimer?.invalidate()
            updateActivity(.idle)
            statusMenuItem.title = "Shift+Fn detected — feedback mode works"
            return
        }
        if didFnChange && !isFnPressed {
            isHotkeyTestActive = false
            hotkeyTestTimer?.invalidate()
            updateActivity(.idle)
            statusMenuItem.title = "Fn detected, but Shift was not held"
        }
    }

    private func setRecording(isEnabled: Bool, isProfileFeedback: Bool) {
        guard isRecording != isEnabled else {
            return
        }
        isRecording = isEnabled
        if isEnabled {
            developerOverlayWatchTimer?.invalidate()
            developerOverlayPanel?.orderOut(nil)
            let startedAt = Date()
            let runID = "\(Int(startedAt.timeIntervalSince1970 * 1_000))-\(UUID().uuidString.prefix(8))"
            let directory = runsURL.appendingPathComponent(runID, isDirectory: true)
            let contextURL = directory.appendingPathComponent("context-input.json")
            do {
                try FileManager.default.createDirectory(
                    at: directory,
                    withIntermediateDirectories: true
                )
                let streamingSession = moonshineTranscriber.flatMap {
                    try? MoonshineStreamingSession(
                        transcriber: $0,
                        queue: moonshineQueue
                    )
                }
                moonshineSession = streamingSession
                nativeRecorder = try NativeRecorder(
                    sourceURL: directory.appendingPathComponent("audio-native.caf"),
                    onAudio: { [weak streamingSession] audio, sampleRate in
                        streamingSession?.addAudio(audio, sampleRate: sampleRate)
                    }
                )
            } catch {
                _ = moonshineSession?.stop()
                moonshineSession = nil
                isRecording = false
                refreshActivity()
                return
            }
            currentRunID = runID
            currentContextURL = contextURL
            currentCreatedAt = ISO8601DateFormatter().string(from: startedAt)
            currentIsProfileFeedback = isProfileFeedback
            updateActivity(isProfileFeedback ? .feedbackRecording : .recording)
            let contextCapture = DispatchWorkItem { [weak self] in
                self?.writeContext(to: contextURL)
            }
            currentContextCapture = contextCapture
            DispatchQueue.global(qos: .userInitiated).async(execute: contextCapture)
            return
        }

        guard let recorder = nativeRecorder,
              let runID = currentRunID,
              let contextURL = currentContextURL,
              let createdAt = currentCreatedAt else {
            refreshActivity()
            return
        }
        let recordingMs = recorder.stop(
            diagnosticsURL: recorder.sourceURL
                .deletingLastPathComponent()
                .appendingPathComponent("capture.json")
        )
        nativeRecorder = nil
        let streamingResult = moonshineSession?.stop()
        moonshineSession = nil
        // Capture begins with recording, so waiting here normally costs
        // nothing and prevents the engine from racing a late atomic write.
        currentContextCapture?.wait()
        currentContextCapture = nil
        currentRunID = nil
        currentContextURL = nil
        currentCreatedAt = nil
        let wasProfileFeedback = currentIsProfileFeedback
        currentIsProfileFeedback = false
        var environment = [
            "TYPR_RUN_ID": runID,
            "TYPR_AUDIO_PATH": recorder.sourceURL.path,
            "TYPR_CONTEXT_FILE": contextURL.path,
            "TYPR_CREATED_AT": createdAt,
            "TYPR_RECORDING_MS": String(recordingMs)
        ]
        if let transcript = streamingResult?.transcript {
            let transcriptURL = recorder.sourceURL
                .deletingLastPathComponent()
                .appendingPathComponent("transcript-moonshine-streaming.txt")
            do {
                try transcript.write(to: transcriptURL, atomically: true, encoding: .utf8)
                environment["TYPR_MOONSHINE_TRANSCRIPT_PATH"] = transcriptURL.path
                environment["TYPR_MOONSHINE_FINALIZE_MS"] = String(
                    streamingResult?.finalizeMs ?? 0
                )
            } catch {
                // The post-stop Moonshine CLI remains a safe fallback.
            }
        }
        guard let process = runEngine(
            wasProfileFeedback ? "process-feedback" : "process",
            environment: environment
        ) else {
            try? FileManager.default.removeItem(at: contextURL)
            refreshActivity()
            return
        }
        activeDictations[ObjectIdentifier(process)] = (process, .processing)
        let runDirectory = recorder.sourceURL.deletingLastPathComponent()
        if !wasProfileFeedback && developerOverlayMenuItem.state == .on {
            watchRunForDeveloperOverlay(runDirectory)
        }
        process.terminationHandler = { [weak self] process in
            try? FileManager.default.removeItem(at: contextURL)
            DispatchQueue.main.async {
                self?.activeDictations.removeValue(forKey: ObjectIdentifier(process))
                if !wasProfileFeedback && self?.developerOverlayMenuItem.state == .on {
                    self?.showDeveloperOverlay(runDirectory)
                }
                self?.refreshRecentDictations()
                self?.refreshActivity()
            }
        }
        refreshActivity()
    }

    private func writeContext(to url: URL) {
        do {
            let data = try JSONEncoder().encode(Accessibility.captureContext())
            try data.write(to: url, options: .atomic)
        } catch {
            // Context improves editing but must never block dictation.
        }
    }

    /*
     * The engine writes the hot-path files atomically. Polling only while a
     * dictation is active avoids an IPC service and surfaces output within one
     * UI tick of insertion rather than waiting for the reference path.
     */
    private func watchRunForDeveloperOverlay(_ directory: URL) {
        developerOverlayWatchTimer?.invalidate()
        var attempts = 0
        developerOverlayWatchTimer = Timer.scheduledTimer(
            withTimeInterval: 0.1,
            repeats: true
        ) { [weak self] timer in
            attempts += 1
            if self?.showDeveloperOverlay(directory) == true || attempts >= 300 {
                timer.invalidate()
            }
        }
    }

    @discardableResult
    private func showDeveloperOverlay(_ directory: URL) -> Bool {
        guard let metadataData = try? Data(
            contentsOf: directory.appendingPathComponent("metadata.json")
        ),
              let metadata = try? JSONDecoder().decode(RunMetadata.self, from: metadataData),
              metadata.hotPathMs != nil,
              let rawTranscript = try? String(
                  contentsOf: directory.appendingPathComponent("transcript.txt"),
                  encoding: .utf8
              ),
              let output = try? String(
                  contentsOf: directory.appendingPathComponent("output.txt"),
                  encoding: .utf8
              ) else {
            return false
        }
        let isMoonshine = metadata.transcriptionModel == "medium-streaming-en"
        let referenceSource = isMoonshine ? "WhisperKit" : "Moonshine"
        let referenceTranscriptURL = directory.appendingPathComponent(
            isMoonshine ? "transcript-whisperkit.txt" : "transcript-moonshine.txt"
        )
        let referenceOutputURL = directory.appendingPathComponent(
            isMoonshine ? "output-whisperkit.txt" : "output-moonshine.txt"
        )
        let contextURL = directory.appendingPathComponent("context.json")
        let contextJSON = try? String(contentsOf: contextURL, encoding: .utf8)
        let data = DeveloperOverlayData(
            runID: directory.lastPathComponent,
            rawTranscript: rawTranscript,
            output: output,
            source: isMoonshine ? "Moonshine" : "WhisperKit",
            transcriptionMs: metadata.transcriptionMs,
            hotPathMs: metadata.hotPathMs,
            timing: metadata.rewriteTiming,
            referenceSource: FileManager.default.fileExists(atPath: referenceOutputURL.path)
                ? referenceSource
                : nil,
            referenceTranscript: try? String(contentsOf: referenceTranscriptURL, encoding: .utf8),
            referenceOutput: try? String(contentsOf: referenceOutputURL, encoding: .utf8),
            referenceTiming: isMoonshine
                ? metadata.whisperKitRewriteTiming
                : metadata.moonshineRewriteTiming,
            context: contextJSON.flatMap {
                try? JSONDecoder().decode(AppContext.self, from: Data($0.utf8))
            },
            contextJSON: contextJSON,
            captureJSON: try? String(
                contentsOf: directory.appendingPathComponent("capture.json"),
                encoding: .utf8
            )
        )
        let view = DeveloperOverlayView(
            data: data,
            close: { [weak self] in
                self?.developerOverlayPanel?.orderOut(nil)
            },
            openDetails: { [weak self] in
                self?.openHistory()
            }
        )
        if developerOverlayPanel == nil {
            let panel = NSPanel(
                contentRect: NSRect(x: 0, y: 0, width: 760, height: 620),
                styleMask: [.nonactivatingPanel, .titled, .closable, .resizable, .fullSizeContentView],
                backing: .buffered,
                defer: false
            )
            panel.title = "Typr Developer"
            panel.titleVisibility = .hidden
            panel.titlebarAppearsTransparent = true
            panel.isFloatingPanel = true
            panel.becomesKeyOnlyIfNeeded = true
            panel.hidesOnDeactivate = false
            panel.level = .floating
            panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
            developerOverlayPanel = panel
        }
        guard let panel = developerOverlayPanel else {
            return false
        }
        panel.contentViewController = NSHostingController(rootView: view)
        if let screen = NSScreen.main {
            let frame = panel.frame
            panel.setFrameOrigin(NSPoint(
                x: screen.visibleFrame.maxX - frame.width - 16,
                y: screen.visibleFrame.maxY - frame.height - 16
            ))
        }
        panel.orderFrontRegardless()
        return true
    }

    private func updateActivity(_ activity: Activity) {
        switch activity {
        case .idle:
            statusItem.button?.image = NSImage(
                systemSymbolName: "waveform",
                accessibilityDescription: "Typr idle"
            )?.withSymbolConfiguration(.init(pointSize: 16, weight: .semibold))
            statusItem.button?.contentTintColor = nil
            statusMenuItem.title = "Hold Fn to dictate"
        case .recording:
            statusItem.button?.image = coloredStatusIcon(
                NSColor(
                    calibratedRed: 0.09,
                    green: 0.55,
                    blue: 0.58,
                    alpha: 1
                )
            )
            statusItem.button?.contentTintColor = nil
            statusMenuItem.title = "Recording…"
        case .feedbackRecording:
            statusItem.button?.image = coloredStatusIcon(
                NSColor(
                    calibratedRed: 0.18,
                    green: 0.42,
                    blue: 0.78,
                    alpha: 1
                )
            )
            statusItem.button?.contentTintColor = nil
            statusMenuItem.title = "Teaching Typr…"
        case .processing:
            statusItem.button?.image = coloredStatusIcon(
                NSColor(
                    calibratedRed: 0.91,
                    green: 0.54,
                    blue: 0.09,
                    alpha: 1
                )
            )
            statusItem.button?.contentTintColor = nil
            statusMenuItem.title = "Processing…"
        }
    }

    /*
     * Status-bar buttons render template images monochromatically. Baking the
     * color into a non-template bitmap keeps activity states visibly colored.
     */
    private func coloredStatusIcon(_ color: NSColor) -> NSImage {
        let waveform = NSImage(size: NSSize(width: 14, height: 14))
        waveform.lockFocus()
        NSImage(
            systemSymbolName: "waveform",
            accessibilityDescription: nil
        )?.withSymbolConfiguration(
            .init(pointSize: 13, weight: .bold)
        )?.draw(in: NSRect(x: 0, y: 0, width: 14, height: 14))
        NSColor.white.setFill()
        NSGraphicsContext.current?.compositingOperation = .sourceIn
        NSBezierPath(rect: NSRect(x: 0, y: 0, width: 14, height: 14)).fill()
        NSGraphicsContext.current?.compositingOperation = .sourceOver
        waveform.unlockFocus()
        waveform.isTemplate = false

        let image = NSImage(size: NSSize(width: 22, height: 22))
        image.lockFocus()
        let circle = NSBezierPath(ovalIn: NSRect(x: 2, y: 2, width: 18, height: 18))
        color.setFill()
        circle.fill()
        NSColor.black.withAlphaComponent(0.2).setStroke()
        circle.lineWidth = 1
        circle.stroke()
        waveform.draw(in: NSRect(x: 4, y: 4, width: 14, height: 14))
        image.unlockFocus()
        image.isTemplate = false
        return image
    }

    private func refreshActivity() {
        if activeDictations.values.contains(where: { value in
            if case .recording = value.activity {
                return true
            }
            if case .feedbackRecording = value.activity {
                return true
            }
            return false
        }) {
            updateActivity(.recording)
            return
        }
        updateActivity(activeDictations.isEmpty ? .idle : .processing)
    }

    func menuWillOpen(_ menu: NSMenu) {
        refreshRecentDictations()
    }

    /*
     * The menu reads archived output instead of holding another in-memory
     * history, so it stays correct across app restarts and failed runs.
     */
    private func refreshRecentDictations() {
        guard let menu = statusItem.menu else {
            return
        }
        recentDictationMenuItems.forEach(menu.removeItem)
        recentDictationMenuItems = []
        guard let separatorIndex = menu.items.firstIndex(of: recentDictationsSeparator) else {
            return
        }
        let directories = ((try? FileManager.default.contentsOfDirectory(
            at: runsURL,
            includingPropertiesForKeys: nil,
            options: [.skipsHiddenFiles]
        )) ?? []).sorted { $0.lastPathComponent > $1.lastPathComponent }

        directories.compactMap { directory -> (String, String)? in
            let outputURL = directory.appendingPathComponent("output.txt")
            guard let text = try? String(contentsOf: outputURL, encoding: .utf8)
                .trimmingCharacters(in: .whitespacesAndNewlines),
                  !text.isEmpty else {
                return nil
            }
            let singleLine = text.replacingOccurrences(
                of: #"\s+"#,
                with: " ",
                options: .regularExpression
            )
            return (singleLine, text)
        }.prefix(5).forEach { singleLine, text in
            let item = NSMenuItem(
                title: singleLine.count > 72 ? "\(singleLine.prefix(71))…" : singleLine,
                action: #selector(copyRecentDictation(_:)),
                keyEquivalent: ""
            )
            item.target = self
            item.representedObject = text
            item.image = NSImage(
                systemSymbolName: "doc.on.doc",
                accessibilityDescription: "Copy dictation"
            )
            recentDictationMenuItems.append(item)
        }

        if recentDictationMenuItems.isEmpty {
            let item = NSMenuItem(title: "No dictations yet", action: nil, keyEquivalent: "")
            item.isEnabled = false
            recentDictationMenuItems.append(item)
        }
        recentDictationMenuItems.enumerated().forEach { offset, item in
            menu.insertItem(item, at: separatorIndex + offset)
        }
    }

    @objc private func copyRecentDictation(_ sender: NSMenuItem) {
        guard let text = sender.representedObject as? String else {
            return
        }
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(text, forType: .string)
    }

    private func runEngine(
        _ command: String,
        environment overrides: [String: String] = [:]
    ) -> Process? {
        guard let executable = Bundle.main.url(
            forResource: "typr",
            withExtension: nil
        ) else {
            statusMenuItem.title = "Typr engine is missing"
            return nil
        }

        let process = Process()
        process.executableURL = executable
        process.arguments = [command]
        var environment = ProcessInfo.processInfo.environment
        environment["HOME"] = FileManager.default.homeDirectoryForCurrentUser.path
        environment["PATH"] =
            "/opt/homebrew/bin:/opt/homebrew/sbin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"
        environment["TYPR_APP_EXECUTABLE"] = Bundle.main.executableURL?.path
        environment["TYPR_SHOW_COMPLETION_NOTIFICATION"] =
            command == "process" && developerOverlayMenuItem.state == .on
                ? "false"
                : completionNotificationsMenuItem.state == .on ? "true" : "false"
        overrides.forEach { environment[$0.key] = $0.value }
        process.environment = environment
        do {
            try process.run()
            return process
        } catch {
            return nil
        }
    }

    func applicationWillTerminate(_ notification: Notification) {
        if let nativeRecorder {
            _ = nativeRecorder.stop(
                diagnosticsURL: nativeRecorder.sourceURL
                    .deletingLastPathComponent()
                    .appendingPathComponent("capture.json")
            )
        }
        _ = moonshineSession?.stop()
        moonshineSession = nil
        currentContextCapture?.wait()
        currentContextCapture = nil
        if let currentContextURL {
            try? FileManager.default.removeItem(at: currentContextURL)
        }
        if let moonshineTranscriber {
            moonshineQueue.sync {
                moonshineTranscriber.close()
            }
        }
    }

    @objc private func toggleLaunchAtLogin() {
        do {
            if SMAppService.mainApp.status == .enabled {
                try SMAppService.mainApp.unregister()
                launchAtLoginMenuItem.state = .off
            } else {
                try SMAppService.mainApp.register()
                launchAtLoginMenuItem.state = .on
            }
        } catch {
            let alert = NSAlert()
            alert.messageText = "Could not update Start at Login"
            alert.informativeText = error.localizedDescription
            alert.runModal()
        }
    }

    @objc private func toggleCompletionNotifications() {
        completionNotificationsMenuItem.state =
            completionNotificationsMenuItem.state == .on ? .off : .on
        UserDefaults.standard.set(
            completionNotificationsMenuItem.state == .on,
            forKey: "showCompletionNotifications"
        )
    }

    @objc private func toggleDeveloperOverlay() {
        developerOverlayMenuItem.state = developerOverlayMenuItem.state == .on ? .off : .on
        UserDefaults.standard.set(
            developerOverlayMenuItem.state == .on,
            forKey: "showDeveloperOverlay"
        )
        if developerOverlayMenuItem.state == .off {
            developerOverlayWatchTimer?.invalidate()
            developerOverlayPanel?.orderOut(nil)
        }
    }

    @objc private func beginHotkeyTest() {
        guard !isRecording else {
            return
        }
        isHotkeyTestActive = true
        hotkeyTestTimer?.invalidate()
        statusMenuItem.title = "Testing: hold Shift+Fn"
        hotkeyTestTimer = Timer.scheduledTimer(withTimeInterval: 10, repeats: false) {
            [weak self] _ in
            guard let self, self.isHotkeyTestActive else {
                return
            }
            self.isHotkeyTestActive = false
            self.updateActivity(.idle)
            self.statusMenuItem.title = "No Shift+Fn event detected"
        }
    }

    @objc private func openAccessibilitySettings() {
        guard let url = URL(
            string: "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
        ) else {
            return
        }
        NSWorkspace.shared.open(url)
    }

    @objc private func openHistory() {
        if historyWindow == nil {
            let window = NSWindow(
                contentRect: NSRect(x: 0, y: 0, width: 1_000, height: 680),
                styleMask: [.titled, .closable, .miniaturizable, .resizable],
                backing: .buffered,
                defer: false
            )
            window.title = "Typr History"
            window.contentViewController = NSHostingController(rootView: HistoryView())
            window.center()
            window.isReleasedWhenClosed = false
            historyWindow = window
        }
        historyWindow?.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
    }

    @objc private func openProfile() {
        if profileWindow == nil {
            let window = NSWindow(
                contentRect: NSRect(x: 0, y: 0, width: 720, height: 560),
                styleMask: [.titled, .closable, .miniaturizable, .resizable],
                backing: .buffered,
                defer: false
            )
            window.title = "Typr Profile"
            window.contentViewController = NSHostingController(rootView: ProfileView())
            window.center()
            window.isReleasedWhenClosed = false
            profileWindow = window
        }
        profileWindow?.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
    }

    @objc private func quit() {
        NSApp.terminate(nil)
    }
}

if CommandLine.arguments.dropFirst().first == "insert" {
    let data = FileHandle.standardInput.readDataToEndOfFile()
    guard let text = String(data: data, encoding: .utf8), !text.isEmpty else {
        exit(1)
    }
    print(Accessibility.insert(text))
    exit(0)
}

let app = NSApplication.shared
private let delegate = AppDelegate()
app.delegate = delegate
app.run()
