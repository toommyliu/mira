using System.Collections.Concurrent;
using System.Reflection;
using System.Reflection.Emit;

namespace Mira.Skua.Bot;

public interface IFallback
{
    object? Invoke(MethodInfo method, object?[] args);
}

public class Surface : DispatchProxy
{
    private static readonly ConcurrentDictionary<(Type, MethodInfo), MethodInfo?> Bound = new();
    private static readonly ConcurrentDictionary<MethodInfo, Func<object, object?[], object?>> Defaults = new();

    private object implementation = null!;
    private Bindings bindings = null!;
    private string area = null!;

    public static T Over<T>(string area, object implementation, Bindings bindings) where T : class
    {
        var proxy = Create<T, Surface>();
        var surface = (Surface)(object)proxy;
        surface.area = area;
        surface.implementation = implementation;
        surface.bindings = bindings;
        return proxy;
    }

    protected override object? Invoke(MethodInfo? method, object?[]? args)
    {
        ArgumentNullException.ThrowIfNull(method);
        args ??= [];
        var target = Bound.GetOrAdd((implementation.GetType(), method), key => Bind(key.Item1, key.Item2));
        if (target is not null)
            return target.Invoke(implementation, BindingFlags.DoNotWrapExceptions, null, args, null);
        if (bindings.TryInvoke(area, method, args, out var bound))
            return bound;
        if (!method.IsAbstract)
            return Defaults.GetOrAdd(method, Default)(this, args);
        if (implementation is IFallback fallback)
            return fallback.Invoke(method, args);
        throw new NotSupportedException($"Mira doesn't support Bot.{area}.{Member(method)} yet.");
    }

    private static MethodInfo? Bind(Type type, MethodInfo method)
    {
        var parameters = method.GetParameters().Select(parameter => parameter.ParameterType).ToArray();
        if (!method.IsGenericMethod)
            return type.GetMethod(method.Name, BindingFlags.Public | BindingFlags.Instance, parameters);
        var arguments = method.GetGenericArguments();
        return type.GetMethods(BindingFlags.Public | BindingFlags.Instance)
            .Where(candidate => candidate.Name == method.Name && candidate.IsGenericMethodDefinition
                && candidate.GetGenericArguments().Length == arguments.Length
                && candidate.GetParameters().Length == parameters.Length)
            .Select(candidate => candidate.MakeGenericMethod(arguments))
            .FirstOrDefault(candidate => candidate.GetParameters().Select(parameter => parameter.ParameterType).SequenceEqual(parameters));
    }

    private static Func<object, object?[], object?> Default(MethodInfo method)
    {
        var types = method.GetParameters().Select(parameter => parameter.ParameterType).ToArray();
        var body = new DynamicMethod(method.Name, typeof(object), [typeof(object), typeof(object?[])], typeof(Surface).Module, true);
        var il = body.GetILGenerator();
        var byRef = types.Select(type => type.IsByRef ? il.DeclareLocal(type.GetElementType()!) : null).ToArray();
        for (int index = 0; index < types.Length; index++)
        {
            if (byRef[index] is not { } local)
                continue;
            LoadArgument(il, index, local.LocalType);
            il.Emit(OpCodes.Stloc, local);
        }
        il.Emit(OpCodes.Ldarg_0);
        il.Emit(OpCodes.Castclass, method.DeclaringType!);
        for (int index = 0; index < types.Length; index++)
        {
            if (byRef[index] is { } local)
                il.Emit(OpCodes.Ldloca, local);
            else
                LoadArgument(il, index, types[index]);
        }
        il.Emit(OpCodes.Call, method);
        var result = il.DeclareLocal(typeof(object));
        if (method.ReturnType != typeof(void))
        {
            if (method.ReturnType.IsValueType)
                il.Emit(OpCodes.Box, method.ReturnType);
            il.Emit(OpCodes.Stloc, result);
        }
        for (int index = 0; index < types.Length; index++)
        {
            if (byRef[index] is not { } local)
                continue;
            il.Emit(OpCodes.Ldarg_1);
            il.Emit(OpCodes.Ldc_I4, index);
            il.Emit(OpCodes.Ldloc, local);
            if (local.LocalType.IsValueType)
                il.Emit(OpCodes.Box, local.LocalType);
            il.Emit(OpCodes.Stelem_Ref);
        }
        il.Emit(OpCodes.Ldloc, result);
        il.Emit(OpCodes.Ret);
        return body.CreateDelegate<Func<object, object?[], object?>>();
    }

    private static readonly MethodInfo ArgumentOf = typeof(Surface).GetMethod(nameof(Argument), BindingFlags.NonPublic | BindingFlags.Static)!;

    private static void LoadArgument(ILGenerator il, int index, Type type)
    {
        il.Emit(OpCodes.Ldarg_1);
        il.Emit(OpCodes.Ldc_I4, index);
        il.Emit(OpCodes.Ldelem_Ref);
        il.Emit(OpCodes.Call, ArgumentOf.MakeGenericMethod(type));
    }

    private static T Argument<T>(object? value) => value is T typed ? typed : default!;

    private static string Member(MethodInfo method) =>
        method.IsSpecialName && method.Name.IndexOf('_') is var split and > 0 ? method.Name[(split + 1)..] : method.Name;
}
