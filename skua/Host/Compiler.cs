using System.Runtime.InteropServices;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json.Nodes;
using Microsoft.CodeAnalysis;
using Microsoft.CodeAnalysis.CSharp;
using Microsoft.CodeAnalysis.Emit;
using Mira.Skua.Protocol;

namespace Mira.Skua;

static class Compiler
{
    private const string Version = "mira-skua-compile-4";
    private const string Include = "//cs_include ";
    private const string Pruned = ".pruned-";
    private static readonly TimeSpan Unused = TimeSpan.FromDays(7);

    private static readonly string[] CoreFiles = ["CoreBots.cs", "CoreFarms.cs", "CoreAdvanced.cs", "CoreStory.cs", "CoreDailies.cs"];

    private static readonly string[] Usings =
    [
        "System", "System.Collections", "System.Collections.Generic", "System.Diagnostics", "System.Drawing",
        "System.Dynamic", "System.Globalization", "System.IO", "System.Linq", "System.Net", "System.Net.Http",
        "System.Reflection", "System.Runtime.CompilerServices", "System.Text", "System.Text.RegularExpressions",
        "System.Threading", "System.Threading.Tasks", "System.Timers", "System.Windows.Forms", "Skua.Core",
        "Skua.Core.Interfaces", "Skua.Core.Models", "Skua.Core.Models.Items", "Skua.Core.Models.Monsters",
        "Skua.Core.Models.Players", "Skua.Core.Models.Quests", "Skua.Core.Models.Servers", "Skua.Core.Models.Shops",
        "Skua.Core.Models.Skills", "Skua.Core.Models.Auras", "Skua.Core.Models.Factions", "Skua.Core.Options",
        "Skua.Core.Utils", "CommunityToolkit.Mvvm.DependencyInjection", "Newtonsoft.Json", "Newtonsoft.Json.Linq",
    ];

    private static readonly CSharpParseOptions Parsing = new(LanguageVersion.Latest);

    public static int Run(string scripts, string cache, string entry)
    {
        JsonObject printed;
        try
        {
            printed = new JsonObject { ["ok"] = Compile(scripts, cache, entry.Replace('\\', '/')) };
        }
        catch (Failure failure)
        {
            printed = new JsonObject { ["error"] = failure.Json() };
        }
        Console.Out.WriteLine(printed.ToJsonString());
        return 0;
    }

    internal static JsonObject Compile(string scripts, string cache, string entry)
    {
        var files = new Files(scripts);
        var closure = files.Closure(files.Find(entry) ?? throw new Failure($"Couldn't find {entry} in the Skua scripts."));
        var references = new Lazy<List<MetadataReference>>(References);
        Built? core = null;
        if (closure.Any(file => CoreFiles.Contains(file.Path)))
        {
            var coreSources = files.Closure(CoreFiles.Select(name => files.Find(name)).OfType<Source>());
            core = Build(cache, RunContext.Core, coreSources, references, "the Skua core scripts");
            closure = closure.Where(file => !coreSources.Any(shared => shared.Path == file.Path)).ToList();
        }
        var entryReferences = core is null
            ? references
            : new Lazy<List<MetadataReference>>(() => [.. references.Value, MetadataReference.CreateFromFile(core.Assembly)]);
        var built = Build(cache, "Entry", closure, entryReferences, entry, core?.Key, closure[^1]);
        Prune(cache);
        return new JsonObject
        {
            ["assembly"] = built.Assembly,
            ["core"] = core?.Assembly,
            ["class"] = built.Class,
            ["options"] = Describe(built, core),
        };
    }

    private static Built Build(
        string cache, string name, List<Source> sources, Lazy<List<MetadataReference>> references, string label,
        string? depends = null, Source? entry = null)
    {
        var key = Key(sources, depends);
        var folder = Path.Combine(cache, key);
        var assembly = Path.Combine(folder, name + ".dll");
        var className = Path.Combine(folder, "class.txt");
        if (File.Exists(assembly) && (entry is null || File.Exists(className)))
        {
            Directory.SetLastWriteTimeUtc(folder, DateTime.UtcNow);
            return new Built(key, assembly, entry is null ? null : File.ReadAllText(className), folder);
        }

        var trees = sources.Select(source => CSharpSyntaxTree.ParseText(source.Text, Parsing, source.Path, Encoding.UTF8)).ToList();
        trees = Rewrite.Stubs(trees);
        trees.Add(CSharpSyntaxTree.ParseText(string.Concat(Usings.Select(name => $"global using {name};\n")), Parsing, "<usings>", Encoding.UTF8));
        var compilation = CSharpCompilation.Create(name, trees, references.Value, new CSharpCompilationOptions(
            OutputKind.DynamicallyLinkedLibrary,
            optimizationLevel: OptimizationLevel.Release,
            deterministic: true,
            concurrentBuild: true,
            nullableContextOptions: NullableContextOptions.Disable));
        compilation = Rewrite.Semantic(compilation);
        string? found = null;
        if (entry is not null)
            found = EntryClass(compilation, compilation.SyntaxTrees.First(tree => tree.FilePath == entry.Path))
                ?? throw new Failure($"{entry.Path} has no class with ScriptMain(IScriptInterface).");

        var staging = $"{folder}.tmp-{Guid.NewGuid():N}";
        Directory.CreateDirectory(staging);
        try
        {
            using (var dll = File.Create(Path.Combine(staging, name + ".dll")))
            using (var pdb = File.Create(Path.Combine(staging, name + ".pdb")))
            {
                var emitted = compilation.Emit(dll, pdb, options: new EmitOptions(debugInformationFormat: DebugInformationFormat.PortablePdb));
                if (!emitted.Success)
                    throw new Failure($"Couldn't compile {label}.", emitted.Diagnostics.Where(diagnostic => diagnostic.Severity == DiagnosticSeverity.Error));
            }
            if (found is not null)
                File.WriteAllText(Path.Combine(staging, "class.txt"), found);
            try
            {
                Directory.Move(staging, folder);
            }
            catch (IOException) when (Directory.Exists(folder))
            {
            }
        }
        finally
        {
            if (Directory.Exists(staging))
                Directory.Delete(staging, recursive: true);
        }
        return new Built(key, assembly, found, folder);
    }

    private static void Prune(string cache)
    {
        var cutoff = DateTime.UtcNow - Unused;
        try
        {
            foreach (var build in new DirectoryInfo(cache).EnumerateDirectories().Where(build => build.Name.Contains(Pruned) || build.LastWriteTimeUtc < cutoff).ToList())
            {
                var doomed = build.Name.Contains(Pruned) ? build.FullName : $"{build.FullName}{Pruned}{Guid.NewGuid():N}";
                try
                {
                    if (doomed != build.FullName)
                        build.MoveTo(doomed);
                    Directory.Delete(doomed, recursive: true);
                }
                catch (Exception error) when (error is IOException or UnauthorizedAccessException)
                {
                }
            }
        }
        catch (Exception error) when (error is IOException or UnauthorizedAccessException)
        {
        }
    }

    private static string? EntryClass(CSharpCompilation compilation, SyntaxTree tree)
    {
        var model = compilation.GetSemanticModel(tree);
        foreach (var method in tree.GetRoot().DescendantNodes().OfType<Microsoft.CodeAnalysis.CSharp.Syntax.MethodDeclarationSyntax>())
        {
            if (method.Identifier.ValueText != "ScriptMain" || method.ParameterList.Parameters.Count != 1)
                continue;
            if (model.GetDeclaredSymbol(method)?.ContainingType is { } type)
                return Metadata(type);
        }
        return null;
    }

    private static string Metadata(INamedTypeSymbol type)
    {
        var name = type.MetadataName;
        if (type.ContainingType is { } outer)
            return Metadata(outer) + "+" + name;
        return type.ContainingNamespace is { IsGlobalNamespace: false } space ? space.ToDisplayString() + "." + name : name;
    }

    private static JsonNode Describe(Built entry, Built? core)
    {
        var described = Path.Combine(entry.Folder, "options.json");
        try
        {
            if (File.Exists(described) && JsonNode.Parse(File.ReadAllText(described)) is JsonObject cached)
                return cached;
        }
        catch (System.Text.Json.JsonException)
        {
        }
        JsonObject options;
        var context = new RunContext("describe", core?.Assembly);
        try
        {
            options = context.Runner().Describe(new Start(0, entry.Assembly, core?.Assembly, entry.Class!, entry.Assembly, []));
        }
        catch (Exception error)
        {
            Console.Error.WriteLine($"Couldn't read the script's options: {error}");
            return new JsonObject { ["fields"] = new JsonArray() };
        }
        finally
        {
            context.Unload();
        }
        var staging = described + ".tmp-" + Environment.ProcessId;
        File.WriteAllText(staging, options.ToJsonString());
        File.Move(staging, described, overwrite: true);
        return options;
    }

    private static List<MetadataReference> References()
    {
        var framework = RuntimeEnvironment.GetRuntimeDirectory();
        var shared = Directory.EnumerateFiles(framework, "*.dll").Where(IsManaged);
        var ours = RunContext.Private.Select(name => Path.Combine(AppContext.BaseDirectory, name + ".dll"));
        return shared.Concat(ours).Select(path => (MetadataReference)MetadataReference.CreateFromFile(path)).ToList();
    }

    private static bool IsManaged(string path)
    {
        try
        {
            System.Reflection.AssemblyName.GetAssemblyName(path);
            return true;
        }
        catch (BadImageFormatException)
        {
            return false;
        }
    }

    private static string Key(List<Source> sources, string? depends)
    {
        using var hash = IncrementalHash.CreateHash(HashAlgorithmName.SHA256);
        void Add(string text) => hash.AppendData(Encoding.UTF8.GetBytes(text + "\0"));
        Add(Version);
        Add(RuntimeInformation.FrameworkDescription);
        Add(depends ?? string.Empty);
        foreach (var path in RunContext.Private)
            Add(Convert.ToHexString(SHA256.HashData(File.ReadAllBytes(Path.Combine(AppContext.BaseDirectory, path + ".dll")))));
        foreach (var source in sources.OrderBy(source => source.Path, StringComparer.Ordinal))
        {
            Add(source.Path);
            Add(source.Text);
        }
        return Convert.ToHexString(hash.GetHashAndReset())[..32].ToLowerInvariant();
    }

    private sealed record Built(string Key, string Assembly, string? Class, string Folder);

    public sealed record Source(string Path, string Text);

    private sealed class Files
    {
        private readonly string root;
        private readonly Dictionary<string, string> paths = new(StringComparer.OrdinalIgnoreCase);

        public Files(string root)
        {
            this.root = root;
            foreach (var file in Directory.EnumerateFiles(root, "*.cs", SearchOption.AllDirectories))
                paths.TryAdd(Path.GetRelativePath(root, file).Replace('\\', '/'), Path.GetRelativePath(root, file).Replace('\\', '/'));
        }

        public Source? Find(string relative)
        {
            var normalized = relative.Trim().Replace('\\', '/');
            if (normalized.StartsWith("Scripts/", StringComparison.OrdinalIgnoreCase))
                normalized = normalized["Scripts/".Length..];
            return paths.TryGetValue(normalized, out var actual)
                ? new Source(actual, File.ReadAllText(Path.Combine(root, actual)))
                : null;
        }

        public List<Source> Closure(IEnumerable<Source> starts)
        {
            var seen = new Dictionary<string, Source>(StringComparer.OrdinalIgnoreCase);
            var order = new List<Source>();
            void Visit(Source source)
            {
                if (!seen.TryAdd(source.Path, source))
                    return;
                foreach (var line in source.Text.Split('\n'))
                {
                    var trimmed = line.Trim();
                    if (!trimmed.StartsWith(Include, StringComparison.Ordinal))
                        continue;
                    var included = trimmed[Include.Length..];
                    Visit(Find(included) ?? throw new Failure($"{source.Path} includes {included.Trim()}, which isn't in the Skua scripts."));
                }
                order.Add(source);
            }
            foreach (var start in starts)
                Visit(start);
            return order;
        }

        public List<Source> Closure(Source entry) => Closure([entry]);
    }

    public sealed class Failure(string message, IEnumerable<Diagnostic>? diagnostics = null) : Exception(message)
    {
        public JsonObject Json() => new()
        {
            ["message"] = Message,
            ["diagnostics"] = new JsonArray((diagnostics ?? []).Select(diagnostic =>
            {
                var span = diagnostic.Location.GetLineSpan();
                return (JsonNode)new JsonObject
                {
                    ["file"] = span.Path,
                    ["line"] = span.StartLinePosition.Line + 1,
                    ["column"] = span.StartLinePosition.Character + 1,
                    ["message"] = diagnostic.GetMessage(),
                };
            }).ToArray()),
        };
    }
}
