using Skua.Core.Models.Items;

namespace Skua.Core.Interfaces;

/// <summary>
/// Defines methods for equipping items from a player's inventory by item ID, name, or inventory item instance.
/// </summary>
/// <remarks>
/// Implementations of this interface allow equipping individual items or multiple items at once,
/// including items that are usable in a specific slot. Methods do nothing if the specified item is not present in the
/// player's inventory. This interface extends <see cref="ICheckEquipped"/>, enabling both equipping and checking equipped
/// status.
/// </remarks>
public interface ICanEquip : ICheckEquipped
{
    /// <summary>
    /// Equips the item with specified <paramref name="id"/>. This will do nothing if the item is not in the player's inventory.
    /// </summary>
    /// <param name="id">ID of the item to equip.</param>
    void EquipItem(int id);

    /// <summary>
    /// Equips the item with specified <paramref name="name"/>. This will do nothing if the item is not in the player's inventory.
    /// </summary>
    /// <param name="name">Name of the item to equip.</param>
    void EquipItem(string name)
    {
        if (TryGetItem(name, out InventoryItem? item))
            EquipItem(item!.ID);
    }

    /// <summary>
    /// Equips items that are usable (slot 6) with specified <paramref name="id"/>. This will do nothing if the item is not in the player's inventory.
    /// </summary>
    /// <param name="id">ID of the item to equip</param>
    void EquipUsableItem(int id)
    {
        if (TryGetItem(id, out InventoryItem? item))
            EquipUsableItem(item);
    }

    /// <summary>
    /// Equips items that are usable (slot 6) with specified <paramref name="name"/>. This will do nothing if the item is not in the player's inventory.
    /// </summary>
    /// <param name="name">Name of the item to equip</param>
    void EquipUsableItem(string name)
    {
        if (TryGetItem(name, out InventoryItem? item))
            EquipUsableItem(item);
    }

    /// <summary>
    /// Equips items that are usable (slot 6) with specified <paramref name="item"/>. This will do nothing if the item is not in the player's inventory.
    /// </summary>
    /// <param name="item">InventoryItem</param>
    void EquipUsableItem(InventoryItem? item);

    /// <summary>
    /// Equips the items with specified <paramref name="names"/>. This will do nothing if the item is not in the player's inventory.
    /// </summary>
    /// <param name="names">Names of the items to equip.</param>
    void EquipItems(params string[] names)
    {
        foreach (string t in names)
        {
            if (TryGetItem(t, out InventoryItem? item))
                EquipItem(item!.ID);
        }
    }

    /// <summary>
    /// Equips the item with specified <paramref name="ids"/>. This will do nothing if the item is not in the player's inventory.
    /// </summary>
    /// <param name="ids">IDs of the items to equip.</param>
    void EquipItems(params int[] ids)
    {
        foreach (int t in ids)
        {
            EquipItem(t);
        }
    }
}