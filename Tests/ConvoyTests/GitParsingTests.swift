import Foundation
import Testing
@testable import Convoy

// MARK: status

@Test func statusParsesStagedAndUnstagedSameFile() {
    let text = "# branch.oid abc\0# branch.head main\0# branch.upstream origin/main\0# branch.ab +2 -1\0" +
        "1 MM N... 100644 100644 100644 aaa bbb Sources/Store.swift\0" +
        "1 A. N... 000000 100644 100644 000 111 new.txt\0" +
        "1 .D N... 100644 100644 000000 222 222 gone.txt\0"
    let s = GitStatusParser.parse(porcelainV2Z: text)
    #expect(s.branch == "main"); #expect(s.upstream == "origin/main"); #expect(s.ahead == 2); #expect(s.behind == 1)
    #expect(s.staged.map(\.path) == ["new.txt", "Sources/Store.swift"])
    #expect(s.unstaged.map(\.path) == ["gone.txt", "Sources/Store.swift"])
    #expect(s.files.first { $0.path == "gone.txt" }?.displayCode == .deleted)
}

@Test func statusParsesRenameUntrackedAndConflicts() {
    let text = "# branch.head (detached)\0" +
        "2 R. N... 100644 100644 100644 aaa aaa R100 new name.txt\0old name.txt\0" +
        "? scratch.txt\0" +
        "u UU N... 100644 100644 100644 100644 a b c both.txt\0"
    let s = GitStatusParser.parse(porcelainV2Z: text)
    #expect(s.detached); #expect(s.branch == "detached")
    let rename = s.files.first { $0.path == "new name.txt" }
    #expect(rename?.originalPath == "old name.txt"); #expect(rename?.index == .renamed); #expect(rename?.hasStaged == true)
    #expect(s.untracked.map(\.path) == ["scratch.txt"])
    #expect(s.conflicted.map(\.path) == ["both.txt"])
    #expect(s.files.first { $0.path == "both.txt" }?.hasStaged == false)
}

// MARK: diff

let sampleDiff = """
diff --git a/a.txt b/a.txt
index 1111111..2222222 100644
--- a/a.txt
+++ b/a.txt
@@ -1,3 +1,4 @@
 one
-two
+TWO
+two and a half
 three
@@ -10,2 +11,2 @@ func x() {
-ten
+TEN
 eleven
\\ No newline at end of file
diff --git a/old.txt b/new.txt
similarity index 90%
rename from old.txt
rename to new.txt
diff --git a/img.png b/img.png
new file mode 100644
Binary files /dev/null and b/img.png differ
diff --git a/gone.txt b/gone.txt
deleted file mode 100644
--- a/gone.txt
+++ /dev/null
@@ -1 +0,0 @@
-bye
"""

@Test func diffParsesMultipleHunksWithLineNumbers() throws {
    let files = DiffParser.parse(sampleDiff)
    #expect(files.count == 4)
    let a = files[0]
    #expect(a.path == "a.txt"); #expect(a.hunks.count == 2); #expect(a.additions == 3); #expect(a.deletions == 2)
    let first = a.hunks[0]
    #expect(first.oldStart == 1 && first.oldCount == 3 && first.newStart == 1 && first.newCount == 4)
    #expect(first.lines.map { $0.kind } == [.context, .removed, .added, .added, .context])
    #expect(first.lines[1].oldNumber == 2 && first.lines[1].newNumber == nil)
    #expect(first.lines[2].newNumber == 2); #expect(first.lines[4].oldNumber == 3 && first.lines[4].newNumber == 4)
    let second = a.hunks[1]
    #expect(second.header.hasSuffix("func x() {"))
    #expect(second.lines.first?.oldNumber == 10); #expect(second.lines.last?.kind == .noNewline)
    #expect(second.lines[2].newNumber == 12)
}

@Test func diffParsesRenameNewDeletedAndBinary() {
    let files = DiffParser.parse(sampleDiff)
    #expect(files[1].isRename && files[1].oldPath == "old.txt" && files[1].newPath == "new.txt" && files[1].hunks.isEmpty)
    #expect(files[2].isBinary && files[2].isNew && files[2].hunks.isEmpty)
    #expect(files[3].isDeleted && files[3].deletions == 1 && files[3].hunks[0].newCount == 0)
}

@Test func diffTruncatesAtMaxLines() {
    let file = DiffParser.parseSingle(sampleDiff, maxLines: 3)
    #expect(file?.truncated == true); #expect(file?.lineCount == 3)
}

@Test func diffPatchRoundTripsHunk() throws {
    let file = try #require(DiffParser.parseSingle(sampleDiff))
    let patch = DiffParser.patch(for: file, hunk: file.hunks[0])
    #expect(patch.hasPrefix("diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1,3 +1,4 @@\n one\n-two\n+TWO\n+two and a half\n three\n"))
    let reparsed = DiffParser.parseSingle(patch)
    #expect(reparsed?.hunks.first?.lines == file.hunks[0].lines)
    #expect(reparsed?.hunks.first?.header == file.hunks[0].header)
}

@Test func untrackedDiffNumbersAllLinesAsAdded() {
    let d = DiffParser.untrackedDiff(path: "n.txt", contents: "a\nb\n")
    #expect(d.isNew && d.additions == 2 && d.hunks[0].lines.map(\.newNumber) == [1, 2])
    let noNewline = DiffParser.untrackedDiff(path: "n.txt", contents: "a")
    #expect(noNewline.hunks[0].lines.last?.kind == .noNewline)
}

@Test func sideBySidePairsRemovedWithAddedAndPadsLeftovers() throws {
    let file = try #require(DiffParser.parseSingle(sampleDiff))
    let rows = SideBySide.rows(for: file.hunks[0])
    // context, (two|TWO), (nil|two and a half), context
    #expect(rows.count == 4)
    #expect(rows[0].left == rows[0].right)
    #expect(rows[1].left?.text == "two" && rows[1].right?.text == "TWO")
    #expect(rows[2].left == nil && rows[2].right?.text == "two and a half")
    #expect(rows[3].left?.text == "three")
}

// MARK: log, name-status, branches

@Test func logParserSplitsRecordsAndParents() {
    let text = "aaaa1111\0aaaa111\0Ann\0ann@x\0\(1_700_000_000)\0Merge it\0p1 p2\0HEAD -> main, origin/main\u{1e}\n" +
        "bbbb2222\0bbbb222\0Bob\0bob@x\0\(1_600_000_000)\0First\0\0\u{1e}\n"
    let commits = GitLogParser.parse(text)
    #expect(commits.count == 2)
    #expect(commits[0].isMerge && commits[0].parents == ["p1", "p2"] && commits[0].refs == ["main", "origin/main"])
    #expect(commits[1].parents.isEmpty && commits[1].refs.isEmpty && commits[1].subject == "First")
    #expect(commits[0].date == Date(timeIntervalSince1970: 1_700_000_000))
}

@Test func nameStatusParsesRenameScore() {
    let files = GitNameStatusParser.parse(z: "M\0a.txt\0R087\0old.txt\0new.txt\0A\0b.txt\0")
    #expect(files.map(\.path) == ["a.txt", "new.txt", "b.txt"])
    #expect(files[1].code == .renamed && files[1].originalPath == "old.txt")
}

@Test func branchParserMarksCurrentAndRemote() {
    let text = "main\0*\0origin/main\0100\nfeature/x\0 \0\0200\norigin/main\0 \0\0100\norigin/HEAD\0 \0\0100\n"
    let branches = GitBranchParser.parse(text)
    #expect(branches.map(\.name) == ["main", "feature/x", "origin/main"])
    #expect(branches[0].isCurrent && branches[0].upstream == "origin/main" && !branches[0].isRemote)
    #expect(!branches[1].isRemote && branches[2].isRemote && branches[2].shortName == "main")
}

// MARK: files

@Test func fileTreeBuildsNestedSortedNodes() {
    let tree = FileTree.build(paths: ["src/b.swift", "README.md", "src/A.swift", "src/ui/View.swift", "a.txt"])
    #expect(tree.map(\.name) == ["src", "a.txt", "README.md"])
    #expect(tree[0].children?.map(\.name) == ["ui", "A.swift", "b.swift"])
    #expect(tree[0].children?[0].children?.first?.path == "src/ui/View.swift")
    let changed = FileTree.changedDirectories([GitFileStatus(path: "src/ui/View.swift", index: .unchanged, worktree: .modified)])
    #expect(changed == ["src", "src/ui"])
    let filtered = FileTree.filter(tree, query: "view")
    #expect(filtered.count == 1 && filtered[0].children?.count == 1)
}

@Test func textInspectorDetectsBinary() {
    #expect(TextFileInspector.isBinary(Data([0x41, 0x00, 0x42])))
    #expect(!TextFileInspector.isBinary("hello".data(using: .utf8)!))
    #expect(TextFileInspector.isImage("x/y.PNG") && TextFileInspector.isMarkdown("README.md"))
}

@Test func numstatParsesRenamesAndBinary() {
    let counts = GitNumstatParser.parse("3\t1\ta.txt\n-\t-\timg.png\n2\t0\tsrc/{old => new}.swift\n1\t1\told => new\n")
    #expect(counts["a.txt"] == LineCounts(added: 3, removed: 1))
    #expect(counts["img.png"] == LineCounts(added: 0, removed: 0))
    #expect(counts["src/new.swift"] == LineCounts(added: 2, removed: 0))
    #expect(counts["new"] == LineCounts(added: 1, removed: 1))
}
