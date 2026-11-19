namespace Skua.Core.Interfaces;

/// <summary>
/// Represents an inventory for a script-controlled house, providing methods to check equipped items.
/// </summary>
/// <remarks>This interface extends <see cref="ICheckEquipped"/>, enabling implementations to determine the
/// equipped state of items within the house inventory context. Intended for use in systems where script-driven inventory
/// management is required.</remarks>
public interface IScriptHouseInv : ICheckEquipped
{
}