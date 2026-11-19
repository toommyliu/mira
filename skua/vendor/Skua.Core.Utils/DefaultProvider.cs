namespace Skua.Core.Utils;

public class DefaultProvider
{
    public static object? GetDefault<T>(Type type)
    {
        return type == null
            ? default(T)
            : type.IsArray
            ? Array.Empty<T>()
            : type == typeof(string) ? string.Empty : typeof(IEnumerable<>).IsAssignableFrom(type) ? Enumerable.Empty<T>() : default(T);
    }
}