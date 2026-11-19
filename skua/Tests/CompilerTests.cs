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
}
