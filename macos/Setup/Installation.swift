import AppKit
import Darwin

struct Installation {
    static let appID = "com.thekucel.notedd"
    static let appName = "Notedd.app"

    let directory: URL
    let notes: URL
    let desktopAlias: Bool

    var destination: URL { directory.appendingPathComponent(Self.appName, isDirectory: true) }

    enum Failure: LocalizedError {
        case missingPayload, invalidPayload, occupied(URL), running, unpack(String)

        var errorDescription: String? {
            switch self {
            case .missingPayload: return "The installer is missing Notedd."
            case .invalidPayload: return "The embedded Notedd app is damaged."
            case .occupied(let url): return "Another file already occupies \(url.path). Choose a different app folder."
            case .running: return "Notedd is still running. Quit Notedd, then try again."
            case .unpack(let reason): return "Couldn’t extract Notedd. \(reason)"
            }
        }
    }

    func install(payload: URL) throws -> String? {
        let files = FileManager.default
        try files.createDirectory(at: directory, withIntermediateDirectories: true)
        try checkDestination()

        // Extract beside the destination so the finished bundle can be swapped in atomically.
        let staging = directory.appendingPathComponent(".notedd-setup-\(UUID().uuidString)", isDirectory: true)
        try files.createDirectory(at: staging, withIntermediateDirectories: false)
        defer { try? files.removeItem(at: staging) }

        let unpack = Process()
        let errors = Pipe()
        unpack.executableURL = URL(fileURLWithPath: "/usr/bin/ditto")
        unpack.arguments = ["-x", "-k", payload.path, staging.path]
        unpack.standardError = errors
        try unpack.run()
        let log = errors.fileHandleForReading.readDataToEndOfFile()
        unpack.waitUntilExit()
        guard unpack.terminationStatus == 0 else { throw Failure.unpack(String(decoding: log, as: UTF8.self)) }

        let candidate = staging.appendingPathComponent(Self.appName, isDirectory: true)
        guard let bundle = Bundle(url: candidate), bundle.bundleIdentifier == Self.appID,
              let executable = bundle.executableURL, files.isExecutableFile(atPath: executable.path) else {
            throw Failure.invalidPayload
        }

        try files.createDirectory(at: notes, withIntermediateDirectories: true)
        if try checkDestination() {
            let result = candidate.withUnsafeFileSystemRepresentation { source in
                destination.withUnsafeFileSystemRepresentation { target in
                    renameatx_np(AT_FDCWD, source, AT_FDCWD, target, UInt32(RENAME_SWAP))
                }
            }
            guard result == 0 else { throw POSIXError(POSIXErrorCode(rawValue: errno) ?? .EIO) }
        } else {
            try files.moveItem(at: candidate, to: destination)
        }

        if desktopAlias {
            do { try createDesktopAlias() }
            catch { return error.localizedDescription }
        }
        return nil
    }

    @discardableResult
    private func checkDestination() throws -> Bool {
        var info = stat()
        let result = destination.withUnsafeFileSystemRepresentation { lstat($0, &info) }
        if result != 0 {
            if errno == ENOENT { return false }
            throw POSIXError(POSIXErrorCode(rawValue: errno) ?? .EIO)
        }
        guard info.st_mode & S_IFMT == S_IFDIR, Bundle(url: destination)?.bundleIdentifier == Self.appID else {
            throw Failure.occupied(destination)
        }
        return true
    }

    private func createDesktopAlias() throws {
        let files = FileManager.default
        let desktop = try files.url(for: .desktopDirectory, in: .userDomainMask, appropriateFor: nil, create: false)
        let alias = desktop.appendingPathComponent("Notedd")
        let existing = try? URL.bookmarkData(withContentsOf: alias)
        if let existing {
            var stale = false
            let target = try? URL(resolvingBookmarkData: existing, options: [.withoutUI], bookmarkDataIsStale: &stale)
            if target?.standardizedFileURL == destination.standardizedFileURL { return }
        }
        var info = stat()
        guard alias.withUnsafeFileSystemRepresentation({ lstat($0, &info) }) != 0 && errno == ENOENT else {
            throw Failure.occupied(alias)
        }
        let bookmark = try destination.bookmarkData(options: .suitableForBookmarkFile)
        try URL.writeBookmarkData(bookmark, to: alias)
    }
}
