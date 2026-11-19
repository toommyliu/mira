using Skua.Core.Models.Items;

namespace Skua.Core.Interfaces;

/// <summary>
/// Defines functionality to determine whether specific items are currently equipped.
/// </summary>
/// <remarks>
/// Implementations of this interface provide methods to check the equipped status of items by name or
/// identifier. This interface extends <see cref="ICanBank"/>, indicating that types implementing it also support
/// banking operations.
/// </remarks>
public interface ICheckEquipped : ICanBank
{
    /// <summary>
    /// Checks if the item with specified <paramref name="name"/> is equipped.
    /// </summary>
    /// <param name="name">Name of the item.</param>
    /// <returns><see langword="true"/> if the item is equipped.</returns>
    bool IsEquipped(string name)
    {
        return TryGetItem(name, out InventoryItem? item) && item!.Equipped;
    }

    /// <summary>
    /// Checks if the item with specified <paramref name="id"/> is equipped.
    /// </summary>
    /// <param name="id">Name of the item.</param>
    /// <returns><see langword="true"/> if the item is equipped.</returns>
    bool IsEquipped(int id)
    {
        return TryGetItem(id, out InventoryItem? item) && item!.Equipped;
    }
}