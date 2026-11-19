namespace Mira.Skua.Tests;

public class CompilerTests
{
    [Fact]
    public void Options_that_could_not_be_read_are_read_again_next_time()
    {
        var root = Directory.CreateTempSubdirectory("mira-skua-compile-");
        try
        {
            var scripts = Directory.CreateDirectory(Path.Combine(root.FullName, "Scripts")).FullName;
            var cache = Path.Combine(root.FullName, "cache");
            var ready = Path.Combine(root.FullName, "ready");
            File.WriteAllText(Path.Combine(scripts, "Flaky.cs"), $$"""
                public class Flaky
                {
                    public List<IOption> Options = new() { new Option<bool>("Collect", "Collect", "", true) };

                    public Flaky()
                    {
                        if (!File.Exists(@"{{ready}}"))
                            throw new InvalidOperationException("The game isn't ready.");
                    }

                    public void ScriptMain(IScriptInterface bot) { }
                }
                """);

            var first = Compiler.Compile(scripts, cache, "Flaky.cs");
            File.WriteAllText(ready, string.Empty);
            var second = Compiler.Compile(scripts, cache, "Flaky.cs");

            Assert.Empty(first["options"]!["fields"]!.AsArray());
            Assert.Equal("Collect", second["options"]!["fields"]![0]!["key"]!.ToString());
        }
        finally
        {
            root.Delete(recursive: true);
        }
    }

    [Fact]
    public void A_compile_deletes_builds_unused_for_a_week_and_keeps_the_ones_it_uses()
    {
        var root = Directory.CreateTempSubdirectory("mira-skua-compile-");
        try
        {
            var scripts = Directory.CreateDirectory(Path.Combine(root.FullName, "Scripts")).FullName;
            var cache = Path.Combine(root.FullName, "cache");
            File.WriteAllText(Path.Combine(scripts, "Idle.cs"), "public class Idle { public void ScriptMain(IScriptInterface bot) { } }");
            var used = Path.GetDirectoryName(Compiler.Compile(scripts, cache, "Idle.cs")["assembly"]!.ToString())!;
            var stale = Directory.CreateDirectory(Path.Combine(cache, "stale")).FullName;
            var recent = Directory.CreateDirectory(Path.Combine(cache, "recent")).FullName;
            var weekAgo = DateTime.UtcNow - TimeSpan.FromDays(8);
            Directory.SetLastWriteTimeUtc(stale, weekAgo);
            Directory.SetLastWriteTimeUtc(used, weekAgo);

            Compiler.Compile(scripts, cache, "Idle.cs");

            Assert.False(Directory.Exists(stale));
            Assert.True(Directory.Exists(recent));
            Assert.True(Directory.Exists(used));
        }
        finally
        {
            root.Delete(recursive: true);
        }
    }

    [Fact]
    public void A_build_that_could_only_be_partly_deleted_is_built_again()
    {
        var root = Directory.CreateTempSubdirectory("mira-skua-compile-");
        try
        {
            var scripts = Directory.CreateDirectory(Path.Combine(root.FullName, "Scripts")).FullName;
            var cache = Path.Combine(root.FullName, "cache");
            File.WriteAllText(Path.Combine(scripts, "Idle.cs"), "public class Idle { public void ScriptMain(IScriptInterface bot) { } }");
            File.WriteAllText(Path.Combine(scripts, "Other.cs"), "public class Other { public void ScriptMain(IScriptInterface bot) { } }");
            var idle = Path.GetDirectoryName(Compiler.Compile(scripts, cache, "Idle.cs")["assembly"]!.ToString())!;
            var locked = Directory.CreateDirectory(Path.Combine(idle, "locked")).FullName;
            File.WriteAllText(Path.Combine(locked, "kept"), string.Empty);
            File.SetUnixFileMode(locked, UnixFileMode.UserRead | UnixFileMode.UserExecute);
            Directory.SetLastWriteTimeUtc(idle, DateTime.UtcNow - TimeSpan.FromDays(8));

            Compiler.Compile(scripts, cache, "Other.cs");
            var leftover = Assert.Single(Directory.EnumerateDirectories(cache), folder => folder.Contains(".pruned-"));
            var again = Compiler.Compile(scripts, cache, "Idle.cs")["assembly"]!.ToString();
            Compiler.Compile(scripts, cache, "Other.cs");
            Assert.True(Directory.Exists(leftover));
            Unlock(root);
            Compiler.Compile(scripts, cache, "Other.cs");

            Assert.True(File.Exists(again));
            Assert.DoesNotContain(Directory.EnumerateDirectories(cache), folder => folder.Contains(".pruned-"));
        }
        finally
        {
            Unlock(root);
            root.Delete(recursive: true);
        }
    }

    private static void Unlock(DirectoryInfo root)
    {
        foreach (var directory in root.EnumerateDirectories("locked", SearchOption.AllDirectories))
            File.SetUnixFileMode(directory.FullName, UnixFileMode.UserRead | UnixFileMode.UserWrite | UnixFileMode.UserExecute);
    }
}
