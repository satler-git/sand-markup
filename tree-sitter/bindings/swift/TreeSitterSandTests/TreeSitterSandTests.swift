import XCTest
import SwiftTreeSitter
import TreeSitterSand

final class TreeSitterSandTests: XCTestCase {
    func testCanLoadGrammar() throws {
        let parser = Parser()
        let language = Language(language: tree_sitter_sand())
        XCTAssertNoThrow(try parser.setLanguage(language),
                         "Error loading Sand grammar")
    }
}
