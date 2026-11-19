using Mira.Skua;

return args switch
{
    ["compile", .. var rest] when Named(rest) is { } named
        && named.GetValueOrDefault("--scripts") is { } scripts
        && named.GetValueOrDefault("--cache") is { } cache
        && named.GetValueOrDefault("--entry") is { } entry => Compiler.Run(scripts, cache, entry),
    ["worker"] => Worker.Run(),
    _ => Usage(),
};

static Dictionary<string, string>? Named(string[] rest) =>
    rest.Length % 2 == 0 ? rest.Chunk(2).ToDictionary(pair => pair[0], pair => pair[1]) : null;

static int Usage()
{
    Console.Error.WriteLine("Usage: Mira.Skua compile --scripts <dir> --cache <dir> --entry <path> | worker");
    return 2;
}
