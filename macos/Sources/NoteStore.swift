import Foundation

struct Note {
    let id: String
    var text: String
    var color = 0
    var modified = Date()
    var opened = Date.distantPast
    fileprivate var original: Data?

    var title: String { NoteddCore.title(text) }

    var recent: Date { max(modified, opened) }

    init(id: String = "note-\(UUID().uuidString.lowercased())", text: String = "") {
        self.id = id
        self.text = text
    }

    init?(id: String, data: Data, modified: Date) {
        guard let decoded = NoteddCore.decode(data) else { return nil }
        self.init(id: id, text: decoded.text)
        color = decoded.color
        self.modified = modified
        original = data
    }
}

struct NoteStore {
    let folder: URL
    private let files = FileManager.default

    func load() throws -> [Note] {
        try files.createDirectory(at: folder, withIntermediateDirectories: true)
        let keys: Set<URLResourceKey> = [.contentModificationDateKey, .isRegularFileKey, .isSymbolicLinkKey]
        return try files.contentsOfDirectory(at: folder, includingPropertiesForKeys: Array(keys), options: [.skipsHiddenFiles])
            .filter { $0.pathExtension == "md" }
            .compactMap { url in
                let values = try url.resourceValues(forKeys: keys)
                guard values.isRegularFile == true, values.isSymbolicLink != true else { return nil }
                return try Note(id: url.deletingPathExtension().lastPathComponent, data: Data(contentsOf: url),
                                modified: values.contentModificationDate ?? .distantPast)
            }
    }

    func save(_ note: inout Note) throws {
        guard let data = try NoteddCore.save(to: file(for: note), original: note.original, text: note.text, color: note.color) else { return }
        let opened = note.opened
        if let saved = Note(id: note.id, data: data, modified: Date()) {
            note = saved
            note.opened = opened
        }
    }

    func trash(_ note: Note) throws {
        guard let original = note.original else { return }
        let url = file(for: note)
        guard try Data(contentsOf: url) == original else { throw CocoaError(.fileWriteFileExists) }
        try files.trashItem(at: url, resultingItemURL: nil)
    }

    func restore(_ note: Note) throws {
        guard let data = note.original else { return }
        try NoteddCore.writeNew(data, to: file(for: note))
    }

    func addImage(_ data: Data, extension ext: String) throws -> String {
        let assets = folder.appendingPathComponent("assets", isDirectory: true)
        try files.createDirectory(at: assets, withIntermediateDirectories: true)
        let name = "image-\(UUID().uuidString.lowercased()).\(ext)"
        try NoteddCore.writeNew(data, to: assets.appendingPathComponent(name))
        return "assets/\(name)"
    }

    private func file(for note: Note) -> URL {
        folder.appendingPathComponent(note.id).appendingPathExtension("md")
    }
}
