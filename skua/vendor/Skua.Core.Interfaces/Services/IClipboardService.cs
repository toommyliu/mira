namespace Skua.Core.Interfaces;

/// <summary>
/// Defines a service for interacting with the system clipboard, enabling applications to set and retrieve text and data
/// in various formats.
/// </summary>
/// <remarks>
/// Implementations of this interface provide clipboard functionality that may be platform-specific.
/// Methods support both standard text operations and custom data formats, allowing for flexible data exchange between
/// applications.
/// </remarks>
public interface IClipboardService
{
    /// <summary>
    /// Sets the displayed text to the specified value.
    /// </summary>
    /// <param name="text">The text to display. Can be null or empty to clear the current text.</param>
    void SetText(string text);

    /// <summary>
    /// Sets the data associated with the specified format.
    /// </summary>
    /// <remarks>If data for the specified format already exists, it will be overwritten. The format string is
    /// typically a predefined or custom format identifier recognized by the application or system.</remarks>
    /// <param name="format">A string that specifies the format with which the data is associated. Cannot be null or empty.</param>
    /// <param name="data">The data object to associate with the specified format. Can be null to remove existing data for the format.</param>
    void SetData(string format, object data);

    /// <summary>
    /// Retrieves the data associated with the specified format.
    /// </summary>
    /// <param name="format">The name of the data format to retrieve. Cannot be null.</param>
    /// <returns>An object containing the data in the specified format, or null if the data is not available in that format.</returns>
    object GetData(string format);

    /// <summary>
    /// Retrieves the current text content.
    /// </summary>
    /// <returns>A string containing the current text. Returns an empty string if no text is available.</returns>
    string GetText();
}