namespace Skua.Core.Interfaces;

/// <summary>
/// Provides methods for checking the presence and quantity of items across player, bank, temporary, and house
/// inventories.
/// </summary>
/// <remarks>
/// Implementations of this interface enable scripts to verify whether specific items exist in sufficient
/// quantity in any supported inventory location. Methods may optionally move items to the player's main inventory if
/// requested. These checks are useful for automating inventory management tasks, such as ensuring required items are
/// available before performing actions or quests.
/// </remarks>
public interface IScriptInventoryHelper
{
    /// <summary>
    /// Check the bank, player, temporary and house inventory for the item
    /// </summary>
    /// <param name="name">Name of the item.</param>
    /// <param name="quantity">Desired quantity.</param>
    /// <param name="moveToInventory">Whether send the item to Inventory.</param>
    /// <returns>Whether the item exists in the desired quantity in the bank, player, temporary and house inventory.</returns>
    bool Check(string name, int quantity = 1, bool moveToInventory = true);

    /// <summary>
    /// Check the bank, player, temporary and house inventory for the item
    /// </summary>
    /// <param name="id">ID of the item.</param>
    /// <param name="quantity">Desired quantity</param>
    /// <param name="moveToInventory">Whether send the item to Inventory.</param>
    /// <returns>Whether the item exists in the desired quantity in the bank, player, temporary and house inventory.</returns>
    bool Check(int id, int quantity = 1, bool moveToInventory = true);

    /// <summary>
    /// Check if the bank/inventory has all listed items.
    /// </summary>
    /// <param name="itemNames">Names of the items to be checked.</param>
    /// <param name="quantity">Desired quantity.</param>
    /// <param name="moveToInventory">Whether send the item to Inventory.</param>
    /// <returns>Returns whether all the items exist in the bank or player inventory.</returns>
    bool HasAll(IEnumerable<string> itemNames, int quantity = 1, bool moveToInventory = true);

    /// <summary>
    /// Check if the bank/inventory has at least 1 of all listed items.
    /// </summary>
    /// <param name="itemNames">Array of names of the items to be checked</param>
    /// <param name="quantity">Desired quantity.</param>
    /// <param name="moveToInventory">Whether send the item to Inventory</param>
    /// <returns>Returns whether at-least 1 of the items exist in the bank or player inventory.</returns>
    bool HasAny(IEnumerable<string> itemNames, int quantity = 1, bool moveToInventory = true);

    /// <summary>
    /// Check if the bank/inventory has all listed items.
    /// </summary>
    /// <param name="itemIds">Names of the items to be checked.</param>
    /// <param name="quantity">Desired quantity.</param>
    /// <param name="moveToInventory">Whether send the item to Inventory.</param>
    /// <returns>Returns whether all the items exist in the bank or player inventory.</returns>
    bool HasAll(IEnumerable<int> itemIds, int quantity = 1, bool moveToInventory = true);

    /// <summary>
    /// Check if the bank/inventory has at least 1 of all listed items.
    /// </summary>
    /// <param name="itemIds">Array of names of the items to be checked</param>
    /// <param name="quantity">Desired quantity.</param>
    /// <param name="moveToInventory">Whether send the item to Inventory</param>
    /// <returns>Returns whether at-least 1 of the items exist in the bank or player inventory.</returns>
    bool HasAny(IEnumerable<int> itemIds, int quantity = 1, bool moveToInventory = true);
}