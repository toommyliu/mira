namespace Skua.Core.Models.Skills;

public enum ClassUseMode
{
    Base,
    Atk,
    Def,
    Farm,
    Solo,
    Supp,
    Dodge,
    Ultra
}

public static class ClassUseModeExtensions
{
    public static string[] ToArray()
    {
        return Enum.GetNames(typeof(ClassUseMode));
    }
}