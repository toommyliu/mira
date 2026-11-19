using Skua.Core.Models.Items;
using Skua.Core.Utils;

namespace Skua.Core.Interfaces;

/// <summary>
/// Defines methods for checking and retrieving items in a player's inventory by name or ID, including quantity checks
/// and stack status.
/// </summary>
/// <remarks>
/// This interface extends <see cref="IItemContainer{T}"/> to provide additional inventory-specific operations, such as
/// verifying item presence, retrieving items, and checking stack limits. Implementations are expected to support
/// efficient lookups by both item name and ID. Methods that return quantities or items will return default values (such
/// as 0 or null) if the specified item does not exist in the inventory.
/// </remarks>
/// <typeparam name="T">The type of item contained in the inventory. Must derive from <see cref="ItemBase"/>.</typeparam>
public interface ICheckInventory<T> : IItemContainer<T> where T : ItemBase
{
    /// <summary>
    /// Checks whether the player has the item with specified <paramref name="name"/> in the desired <paramref name="quantity"/>.
    /// </summary>
    /// <param name="name">Name of the item to check for.</param>
    /// <param name="quantity">Desired quantity of the item to check for.</param>
    /// <returns><see langword="true"/> if the player has the specified item stack.</returns>
    bool Contains(string name, int quantity = 1)
    {
        return quantity == 0 || Items.Contains(i => i.Name == name && (i.Quantity >= quantity || i.Category == ItemCategory.Class));
    }

    /// <summary>
    /// Checks whether the player has the item with specified <paramref name="id"/> in the desired <paramref name="quantity"/>.
    /// </summary>
    /// <param name="id">ID of the item to check for.</param>
    /// <param name="quantity">Desired quantity of the item to check for.</param>
    /// <returns><see langword="true"/> if the player has the specified item stack.</returns>
    bool Contains(int id, int quantity = 1)
    {
        return quantity == 0 || Items.Contains(i => i.ID == id && (i.Quantity >= quantity || i.Category == ItemCategory.Class));
    }

    /// <summary>
    /// Checks whether the player has all or any the items with specified <paramref name="names"/> in the desired <paramref name="quantity"/>.
    /// </summary>
    /// <param name="names">Names of the items to check for.</param>
    /// <param name="quantity">Quantity of the items to check for.</param>
    /// <param name="needAll">Whether you need all items specified.</param>
    /// <returns><see langword="true"/> if the <see cref="InventoryItem">inventory </see> contains the specified items.</returns>
    bool Contains(bool needAll, int quantity, params string[] names)
    {
        foreach (string t in names)
        {
            bool hasItem = Contains(t, quantity);

            switch (hasItem)
            {
                case true when !needAll:
                    return true;
                case true when needAll:
                    continue;
                case false when needAll:
                    return false;
            }
        }
        return needAll;
    }

    /// <summary>
    /// Checks whether the player has all or any the items with specified <paramref name="ids"/> in the deisred <paramref name="quantity"/>.
    /// </summary>
    /// <param name="ids">IDs of the items to check for.</param>
    /// <param name="quantity">Quantity of the items to check for.</param>
    /// <param name="needAll">Whether you need all items specified.</param>
    /// <returns><see langword="true"/> if the <see cref="InventoryItem">inventory </see> contains the specified items.</returns>
    bool Contains(bool needAll, int quantity, params int[] ids)
    {
        foreach (int t in ids)
        {
            bool hasItem = Contains(t, quantity);

            switch (hasItem)
            {
                case true when !needAll:
                    return true;
                case true when needAll:
                    continue;
                case false when needAll:
                    return false;
            }
        }
        return needAll;
    }

    /// <summary>
    /// Attempts to get the item by the given <paramref name="name"/> and sets the out parameter to its value.
    /// </summary>
    /// <param name="name">Name of the item to get.</param>
    /// <param name="item">The item object to set.</param>
    /// <returns><see langword="true"/> if the item with the given <paramref name="name"/> exists in the <see cref="InventoryItem">inventory</see>.</returns>
    bool TryGetItem(string name, out T? item)
    {
        return (item = GetItem(name)) is not null;
    }

    /// <summary>
    /// Attempts to get the item by the given <paramref name="id"/> and sets the out parameter to its value.
    /// </summary>
    /// <param name="id">ID of the item to get.</param>
    /// <param name="item">The item object to set.</param>
    /// <returns><see langword="true"/> if the item with the given <paramref name="id"/> exists in the <see cref="InventoryItem">inventory</see>.</returns>
    bool TryGetItem(int id, out T? item)
    {
        return (item = GetItem(id)) is not null;
    }

    /// <summary>
    /// Get the <see cref="T">item</see> with the specified <paramref name="name"/>.
    /// </summary>
    /// <param name="name">Name of T item to get.</param>
    /// <returns>The <see cref="T"/> with the specified <paramref name="name"/> or <see langword="null"/> if it doesn't exist.</returns>
    T? GetItem(string name)
    {
        return Items?.Find(x => x.Name == name);
    }

    /// <summary>
    /// Get the <see cref="T">item</see> with the specified <paramref name="id"/>.
    /// </summary>
    /// <param name="id">ID of the item to get.</param>
    /// <returns>The <see cref="T"/> with the specified <paramref name="id"/> or <see langword="null"/> if it doesn't exist.</returns>
    T? GetItem(int id)
    {
        return Items?.Find(x => x.ID == id);
    }

    /// <summary>
    /// Gets the quantity of the item with specified <paramref name="name"/>.
    /// </summary>
    /// <param name="name">Name of the item.</param>
    /// <returns>The quantity of the specified item.</returns>
    int GetQuantity(string name)
    {
        return TryGetItem(name, out T? item) ? item!.Quantity : 0;
    }

    /// <summary>
    /// Gets the quantity of the item with specified <paramref name="id"/>.
    /// </summary>
    /// <param name="id">ID of the item.</param>
    /// <returns>The quantity of the specified item.</returns>
    int GetQuantity(int id)
    {
        return TryGetItem(id, out T? item) ? item!.Quantity : 0;
    }

    /// <summary>
    /// Checks if the item with the specified <paramref name="name"/> is max stacked.
    /// </summary>
    /// <param name="name">Name of the item.</param>
    /// <returns><see langword="true"/> if the item is at max stack.</returns>
    bool IsMaxStack(string name)
    {
        return TryGetItem(name, out T? item) && item!.Quantity >= item!.MaxStack;
    }

    /// <summary>
    /// Checks if the item with the specified <paramref name="id"/> is max stacked.
    /// </summary>
    /// <param name="id">ID of the item.</param>
    /// <returns><see langword="true"/> if the item is at max stack.</returns>
    bool IsMaxStack(int id)
    {
        return TryGetItem(id, out T? item) && item!.Quantity >= item!.MaxStack;
    }
}