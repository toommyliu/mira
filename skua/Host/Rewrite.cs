using System.Text;
using Microsoft.CodeAnalysis;
using Microsoft.CodeAnalysis.CSharp;
using Microsoft.CodeAnalysis.CSharp.Syntax;

namespace Mira.Skua;

static class Rewrite
{
    private static readonly string[] Emptied = ["AprilFools", "UserSpecificMessages", "CollectData", "SkuaVersionCheckerAsync", "AutoReport"];

    public static List<SyntaxTree> Stubs(List<SyntaxTree> trees) => trees.Select(tree =>
    {
        if (tree.FilePath != "CoreBots.cs")
            return tree;
        var root = tree.GetRoot();
        var methods = root.DescendantNodes().OfType<ClassDeclarationSyntax>()
            .Where(type => type.Identifier.ValueText == "CoreBots")
            .SelectMany(type => type.Members.OfType<MethodDeclarationSyntax>())
            .Where(method => Emptied.Contains(method.Identifier.ValueText))
            .ToList();
        var missing = Emptied.Except(methods.Select(method => method.Identifier.ValueText)).ToList();
        if (missing.Count > 0)
            throw new Compiler.Failure($"CoreBots changed in a way Mira hasn't checked ({string.Join(", ", missing)} is gone). Update Mira.");
        return CSharpSyntaxTree.Create((CSharpSyntaxNode)root.ReplaceNodes(methods, (method, _) => Empty(method)), (CSharpParseOptions)tree.Options, tree.FilePath, Encoding.UTF8);
    }).ToList();

    private static MethodDeclarationSyntax Empty(MethodDeclarationSyntax method)
    {
        var returnsValue = method.ReturnType is not PredefinedTypeSyntax { Keyword.ValueText: "void" }
            && !method.Modifiers.Any(SyntaxKind.AsyncKeyword);
        var body = returnsValue
            ? SyntaxFactory.Block(SyntaxFactory.ReturnStatement(SyntaxFactory.LiteralExpression(SyntaxKind.DefaultLiteralExpression)))
            : SyntaxFactory.Block();
        return method.WithBody(body).WithExpressionBody(null).WithSemicolonToken(default);
    }

    private static readonly CatchClauseSyntax Reported = SyntaxFactory.CatchClause(
        SyntaxFactory.CatchDeclaration(SyntaxFactory.ParseTypeName("global::System.Exception "), SyntaxFactory.Identifier("miraFault")),
        null,
        SyntaxFactory.Block(SyntaxFactory.ParseStatement("global::Mira.Skua.Bot.Faults.Report(miraFault);")));

    public static CSharpCompilation Semantic(CSharpCompilation compilation)
    {
        var global = compilation.GlobalNamespace;
        foreach (var original in compilation.SyntaxTrees.ToList())
        {
            var tree = WithoutMissingUsings(original, global);
            if (tree != original)
                compilation = compilation.ReplaceSyntaxTree(original, tree);
            var text = tree.GetText().ToString();
            if (!text.Contains("Start") && !text.Contains("async") && !text.Contains("Thread"))
                continue;
            var model = compilation.GetSemanticModel(tree);
            var changes = new Dictionary<SyntaxNode, Func<SyntaxNode, SyntaxNode>>();
            foreach (var node in tree.GetRoot().DescendantNodes())
            {
                if (Change(node, model) is { } change)
                    changes[node] = change;
            }
            if (changes.Count == 0)
                continue;
            var rewritten = tree.GetRoot().ReplaceNodes(changes.Keys, (original, current) => changes[original](current));
            compilation = compilation.ReplaceSyntaxTree(tree, CSharpSyntaxTree.Create((CSharpSyntaxNode)rewritten, (CSharpParseOptions)tree.Options, tree.FilePath, Encoding.UTF8));
        }
        return compilation;
    }

    private static SyntaxTree WithoutMissingUsings(SyntaxTree tree, INamespaceSymbol global)
    {
        var root = tree.GetCompilationUnitRoot();
        var missing = root.Usings
            .Where(directive => directive.Alias is null && directive.StaticKeyword.IsKind(SyntaxKind.None)
                && directive.Name?.ToString() is { } name && name.StartsWith("Skua.", StringComparison.Ordinal)
                && Namespace(global, name) is null)
            .ToList();
        if (missing.Count == 0)
            return tree;
        var kept = root.RemoveNodes(missing, SyntaxRemoveOptions.KeepExteriorTrivia)!;
        return CSharpSyntaxTree.Create(kept, (CSharpParseOptions)tree.Options, tree.FilePath, Encoding.UTF8);
    }

    private static INamespaceSymbol? Namespace(INamespaceSymbol global, string name) =>
        name.Split('.').Aggregate((INamespaceSymbol?)global, (space, part) => space?.GetNamespaceMembers().FirstOrDefault(member => member.Name == part));

    private static Func<SyntaxNode, SyntaxNode>? Change(SyntaxNode node, SemanticModel model)
    {
        switch (node)
        {
            case ExpressionStatementSyntax { Expression: InvocationExpressionSyntax launch } when IsProcessStart(launch, model):
                return current => SyntaxFactory.EmptyStatement().WithTriviaFrom(current);
            case InvocationExpressionSyntax launch when launch.Parent is not ExpressionStatementSyntax && IsProcessStart(launch, model):
                var method = (IMethodSymbol)model.GetSymbolInfo(launch).Symbol!;
                var nothing = method.IsStatic
                    ? SyntaxFactory.ParseExpression("((global::System.Diagnostics.Process)null)")
                    : SyntaxFactory.LiteralExpression(SyntaxKind.FalseLiteralExpression);
                return current => SyntaxFactory.ParenthesizedExpression(nothing).WithTriviaFrom(current);
            case ArgumentSyntax argument when ThreadStart(argument, model) is { } start:
                return current => Guard((ArgumentSyntax)current, start);
            case MethodDeclarationSyntax declaration when declaration.Modifiers.Any(SyntaxKind.AsyncKeyword)
                && IsAsyncVoid(model.GetDeclaredSymbol(declaration)):
                return current => Guard((MethodDeclarationSyntax)current);
            case LocalFunctionStatementSyntax local when local.Modifiers.Any(SyntaxKind.AsyncKeyword)
                && IsAsyncVoid(model.GetDeclaredSymbol(local)):
                return current => Guard((LocalFunctionStatementSyntax)current);
            case AnonymousFunctionExpressionSyntax lambda when lambda.AsyncKeyword.IsKind(SyntaxKind.AsyncKeyword)
                && IsAsyncVoid(model.GetSymbolInfo(lambda).Symbol):
                return current => Guard((AnonymousFunctionExpressionSyntax)current);
            default:
                return null;
        }
    }

    private static bool IsProcessStart(InvocationExpressionSyntax launch, SemanticModel model) =>
        launch.Expression is MemberAccessExpressionSyntax { Name.Identifier.ValueText: "Start" } or IdentifierNameSyntax { Identifier.ValueText: "Start" }
        && model.GetSymbolInfo(launch).Symbol is IMethodSymbol method
        && method.ContainingType.ToDisplayString() == "System.Diagnostics.Process";

    private static bool IsAsyncVoid(ISymbol? symbol) => symbol is IMethodSymbol { IsAsync: true, ReturnsVoid: true };

    private static string? ThreadStart(ArgumentSyntax argument, SemanticModel model)
    {
        if (argument.Parent?.Parent is not BaseObjectCreationExpressionSyntax creation
            || model.GetSymbolInfo(creation).Symbol is not IMethodSymbol { MethodKind: MethodKind.Constructor } constructor
            || constructor.ContainingType.ToDisplayString() != "System.Threading.Thread")
            return null;
        var index = ((ArgumentListSyntax)argument.Parent).Arguments.IndexOf(argument);
        var type = index < constructor.Parameters.Length ? constructor.Parameters[index].Type.ToDisplayString() : null;
        return type is "System.Threading.ThreadStart" or "System.Threading.ParameterizedThreadStart" ? "global::" + type : null;
    }

    private static ArgumentSyntax Guard(ArgumentSyntax argument, string start) => argument.WithExpression(
        SyntaxFactory.InvocationExpression(
            SyntaxFactory.ParseExpression("global::Mira.Skua.Bot.Faults.Guard"),
            SyntaxFactory.ArgumentList(SyntaxFactory.SingletonSeparatedList(SyntaxFactory.Argument(
                SyntaxFactory.CastExpression(SyntaxFactory.ParseTypeName(start), SyntaxFactory.ParenthesizedExpression(argument.Expression)))))));

    private static BlockSyntax Guard(BlockSyntax body) =>
        SyntaxFactory.Block(SyntaxFactory.TryStatement(body, SyntaxFactory.SingletonList(Reported), null));

    private static BlockSyntax Guard(ExpressionSyntax body) =>
        Guard(SyntaxFactory.Block(SyntaxFactory.ExpressionStatement(body)));

    private static MethodDeclarationSyntax Guard(MethodDeclarationSyntax method) => method.Body is { } body
        ? method.WithBody(Guard(body))
        : method.WithBody(Guard(method.ExpressionBody!.Expression)).WithExpressionBody(null).WithSemicolonToken(default);

    private static LocalFunctionStatementSyntax Guard(LocalFunctionStatementSyntax local) => local.Body is { } body
        ? local.WithBody(Guard(body))
        : local.WithBody(Guard(local.ExpressionBody!.Expression)).WithExpressionBody(null).WithSemicolonToken(default);

    private static AnonymousFunctionExpressionSyntax Guard(AnonymousFunctionExpressionSyntax lambda) => lambda.Block is { } body
        ? lambda.WithBlock(Guard(body))
        : lambda.WithBlock(Guard(lambda.ExpressionBody!)).WithExpressionBody(null);
}
