using Skua.Core.Models.Items;

namespace Skua.Core.Interfaces;

/// <summary>
/// Defines a container that provides access to a collection of items of a specified type.
/// </summary>
/// <typeparam name="T">The type of items contained in the container. Must derive from <see cref="ItemBase"/>.</typeparam>
public interface IItemContainer<T> where T : ItemBase
{
    /// <summary>
    /// A list of items in this inventory.
    /// </summary>
    List<T> Items { get; }
}