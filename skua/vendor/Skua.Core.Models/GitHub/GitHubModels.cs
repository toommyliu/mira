using Newtonsoft.Json;

namespace Skua.Core.Models.GitHub;

public class GitHubCommit
{
    [JsonProperty("sha")]
    public string Sha { get; set; } = string.Empty;

    [JsonProperty("commit")]
    public CommitDetails? Commit { get; set; }
}

public class CommitDetails
{
    [JsonProperty("message")]
    public string Message { get; set; } = string.Empty;

    [JsonProperty("author")]
    public CommitAuthor? Author { get; set; }
}

public class CommitAuthor
{
    [JsonProperty("name")]
    public string Name { get; set; } = string.Empty;

    [JsonProperty("date")]
    public DateTime Date { get; set; }
}

public class GitHubCompare
{
    [JsonProperty("base_commit")]
    public GitHubCommit? BaseCommit { get; set; }

    [JsonProperty("commits")]
    public List<GitHubCommit> Commits { get; set; } = new();

    [JsonProperty("files")]
    public List<GitHubFile> Files { get; set; } = new();
}

public class GitHubFile
{
    [JsonProperty("sha")]
    public string Sha { get; set; } = string.Empty;

    [JsonProperty("filename")]
    public string FileName { get; set; } = string.Empty;

    [JsonProperty("status")]
    public string Status { get; set; } = string.Empty;

    [JsonProperty("additions")]
    public int Additions { get; set; }

    [JsonProperty("deletions")]
    public int Deletions { get; set; }

    [JsonProperty("changes")]
    public int Changes { get; set; }
}
