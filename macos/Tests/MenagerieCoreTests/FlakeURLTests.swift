import Foundation
import Testing

@testable import MenagerieCore

@Test func flakeURLSeparatesPathRefAndAttribute() throws {
  let path = FileManager.default.temporaryDirectory.appendingPathComponent("space # & repo").path
  let branch = "feature/#&test"
  let attr = "host#&name"
  let url = try #require(
    URLComponents(string: Deployment.flakeURL(path: path, branch: branch, attribute: attr)))
  #expect(url.path == path && url.fragment == attr)
  #expect(url.queryItems == [URLQueryItem(name: "ref", value: "refs/heads/" + branch)])
  #expect(
    URLComponents(string: Deployment.flakeURL(path: ".", branch: branch, attribute: attr))!.path
      .hasPrefix("/"))
}
@Test func remoteFlakePathEscapesSpacesAndDelimiters() throws {
  let repo = try Repository()
  defer { repo.remove() }
  let path = repo.path + "/space # & repo"
  try FileManager.default.createDirectory(atPath: path, withIntermediateDirectories: true)
  let result = try Runner.run(
    Command(
      "/bin/sh", ["-c", Deployment.remoteFlakeAssignment + "\nprintf '%s' \"$flake\""],
      directory: path)
  ).checked()
  let url = try #require(URLComponents(string: result))
  #expect(try Runner.run(Command("/bin/test", [url.path, "-ef", path])).status == 0)
  #expect(url.queryItems?.first?.name == "rev")
}
