using System.Collections.Immutable;
using System.ComponentModel;

namespace Skua.Core.Interfaces;

/// <summary>
/// Defines the contract for a map service that provides access to map-related information, cell and pad data, and
/// navigation operations within a mapping context.
/// </summary>
/// <remarks>
/// Implementations of this interface are expected to support property change notifications via the
/// <see cref="INotifyPropertyChanged"/> interface. The interface exposes properties for retrieving map and cell information, as well
/// as methods for performing navigation actions such as travel and jump. Consumers should refer to the specific
/// implementation for details on supported operations and any additional constraints.</remarks>
public interface IMapService : INotifyPropertyChanged
{
    /// <summary>
    /// Gets the name of the map associated with this instance.
    /// </summary>
    string MapName { get; }

    /// <summary>
    /// Gets the value of the cell as a string.
    /// </summary>
    string Cell { get; }

    /// <summary>
    /// Gets the padding string used for formatting or alignment operations.
    /// </summary>
    string Pad { get; }

    /// <summary>
    /// Gets the collection of cell values for the current row.
    /// </summary>
    List<string> Cells { get; }

    /// <summary>
    /// Gets the collection of pad names associated with the current instance.
    /// </summary>
    ImmutableList<string> Pads { get; }

    /// <summary>
    /// Gets or sets a value indicating whether a private room should be used for the session.
    /// </summary>
    bool UsePrivateRoom { get; set; }

    /// <summary>
    /// Gets or sets the number assigned to the private room.
    /// </summary>
    int PrivateRoomNumber { get; set; }

    /// <summary>
    /// Retrieves the current location information, including the map name, cell, and pad.
    /// </summary>
    /// <returns>A tuple containing the current map name, cell, and pad. Each element is a string representing the respective
    /// location component.</returns>
    (string mapName, string cell, string pad) GetCurrentLocation();

    /// <summary>
    /// Gets the identifier of the current cell and its associated pad.
    /// </summary>
    /// <returns>A tuple containing the current cell identifier and the associated pad. The first element is the cell identifier;
    /// the second element is the pad name. Both values are strings.</returns>
    (string cell, string pad) GetCurrentCell();

    /// <summary>
    /// Performs a travel operation using the specified information object.
    /// </summary>
    /// <param name="info">An object containing information required to perform the travel operation. The expected type and required
    /// properties depend on the implementation. Can be null if the operation supports it.</param>
    void Travel(object? info);

    /// <summary>
    /// Performs a jump operation from the specified cell using the given pad.
    /// </summary>
    /// <param name="cell">The identifier of the cell from which the jump is initiated. Cannot be null or empty.</param>
    /// <param name="pad">The identifier of the pad to use for the jump. Cannot be null or empty.</param>
    void Jump(string cell, string pad);
}