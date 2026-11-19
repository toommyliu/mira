using Skua.Core.Models.Items;

namespace Skua.Core.Interfaces;

/// <summary>
/// Represents a temporary inventory for script operations that supports inventory checking functionality.
/// </summary>
/// <remarks>
/// This interface extends <see cref="ICheckInventory{T}"/>, enabling scripts to query or validate
/// the presence of items in a temporary inventory context. Implementations may be used for transient or script-driven
/// item management scenarios.
/// </remarks>
public interface IScriptTempInv : ICheckInventory<ItemBase>
{
}