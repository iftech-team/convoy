import Foundation

/// Looks at directory names and project markers only. Never follows symlinks or
/// scans dependency/build trees. Stops at a repository boundary by default.
struct ProjectDiscovery {
    static let excluded: Set<String> = [
        "node_modules", "vendor", "target", "dist", "build", "DerivedData",
        "Pods", "Carthage", "venv", "env", "__pycache__", "coverage"
    ]
    static let markers: Set<String> = [
        ".git", "package.json", "Cargo.toml", "Package.swift", "composer.json",
        "pyproject.toml", "setup.py", "requirements.txt", "go.mod", "pubspec.yaml",
        "Gemfile", "pom.xml", "build.gradle", "build.gradle.kts", "CMakeLists.txt",
        "mix.exs", "deno.json", "deno.jsonc"
    ]

    func scan(_ root: URL, forceGroup: Bool = false) throws -> [Project] {
        var remaining = 3000
        func visit(_ url: URL, parent: UUID?, depth: Int, isRoot: Bool) throws -> [Project] {
            guard remaining > 0, depth <= 10 else {
                throw NSError(domain: "Convoy", code: 3, userInfo: [NSLocalizedDescriptionKey: "This folder is too large to import at once. Choose a smaller project collection."])
            }
            remaining -= 1
            let entries = try FileManager.default.contentsOfDirectory(at: url, includingPropertiesForKeys: [.isDirectoryKey, .isSymbolicLinkKey], options: [])
            let names = Set(entries.map(\.lastPathComponent))
            let isProject = !names.isDisjoint(with: Self.markers) || names.contains { $0.hasSuffix(".xcodeproj") || $0.hasSuffix(".sln") || $0.hasSuffix(".csproj") }
            var item = Project(name: url.lastPathComponent, path: url.standardizedFileURL.path, parentID: parent)
            if isProject && !(isRoot && forceGroup) { return [item] }
            var descendants: [Project] = []
            for child in entries.sorted(by: { $0.lastPathComponent.localizedStandardCompare($1.lastPathComponent) == .orderedAscending }) {
                let name = child.lastPathComponent
                guard !name.hasPrefix("."), !Self.excluded.contains(name) else { continue }
                let values = try child.resourceValues(forKeys: [.isDirectoryKey, .isSymbolicLinkKey])
                guard values.isDirectory == true, values.isSymbolicLink != true else { continue }
                descendants += try visit(child, parent: item.id, depth: depth + 1, isRoot: false)
            }
            if !descendants.isEmpty || (isRoot && forceGroup) {
                item.group = true
                return [item] + descendants
            }
            return isRoot ? [item] : []
        }
        return try visit(root.standardizedFileURL, parent: nil, depth: 0, isRoot: true)
    }
}
