using System.Reflection;
using Skua.Core.Interfaces;

namespace Mira.Skua.Tests;

public class DispatchTests
{
    private static readonly Type[] Broken =
    [
        typeof(InvalidProgramException), typeof(InvalidCastException), typeof(TargetParameterCountException),
        typeof(MissingMethodException), typeof(AmbiguousMatchException),
    ];

    private static readonly HashSet<string> Waiting = ["Sleep", "SleepAsync", "ForItem", "EnsureAccept", "EnsureRelogin"];

    [Fact]
    public void Every_bot_member_dispatches_to_something_that_can_run()
    {
        var bot = Offline.Bot();
        var broken = new List<string>();
        foreach (var area in typeof(IScriptInterface).GetProperties(BindingFlags.Public | BindingFlags.Instance).Where(property => property.PropertyType.IsInterface && property.Name != "Wait"))
        {
            var target = area.GetValue(bot)!;
            var methods = new[] { area.PropertyType }.Concat(area.PropertyType.GetInterfaces()).SelectMany(type => type.GetMethods());
            foreach (var method in methods.Where(method => !method.IsGenericMethodDefinition && !Waiting.Contains(method.Name)
                && !method.Name.StartsWith("add_") && !method.Name.StartsWith("remove_")))
            {
                try
                {
                    method.Invoke(target, BindingFlags.DoNotWrapExceptions, null, method.GetParameters().Select(Sample).ToArray(), null);
                }
                catch (Exception error) when (Broken.Contains(error.GetType()))
                {
                    broken.Add($"{area.Name}.{method.Name}: {error.Message}");
                }
                catch (Exception)
                {
                }
            }
        }
        Assert.Empty(broken);
    }

    private static object? Sample(ParameterInfo parameter)
    {
        var type = parameter.ParameterType.IsByRef ? parameter.ParameterType.GetElementType()! : parameter.ParameterType;
        if (parameter.HasDefaultValue && parameter.DefaultValue is not DBNull)
            return parameter.DefaultValue;
        if (type == typeof(string))
            return "Gem";
        if (type == typeof(int))
            return 1;
        if (type == typeof(Type))
            return typeof(string);
        if (type.IsArray)
            return Array.CreateInstance(type.GetElementType()!, 0);
        if (type.IsEnum)
            return Enum.GetValues(type).GetValue(0);
        if (type.IsValueType)
            return Activator.CreateInstance(type);
        if (type.IsGenericType && type.GetGenericTypeDefinition() == typeof(IEnumerable<>))
            return Array.CreateInstance(type.GetGenericArguments()[0], 0);
        return type.GetConstructor(Type.EmptyTypes)?.Invoke(null);
    }
}
