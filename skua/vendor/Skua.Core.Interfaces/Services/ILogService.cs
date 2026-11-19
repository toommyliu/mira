using Skua.Core.Models;

namespace Skua.Core.Interfaces;

public interface ILogService
{
    /// <summary>
    /// Writes a debug-level log message for diagnostic purposes.
    /// </summary>
    /// <param name="message">The message to log. Cannot be null.</param>
    void DebugLog(string message);

    /// <summary>
    /// Writes a message to the script log output.
    /// </summary>
    /// <param name="message">The message to write to the log. Cannot be null.</param>
    void ScriptLog(string message);

    /// <summary>
    /// Displays the specified log message in a prominent or temporary manner to draw immediate attention.
    /// </summary>
    /// <remarks>Use this method to highlight important log entries that require immediate visibility, such as
    /// errors or critical notifications. The exact presentation may vary depending on the implementation.</remarks>
    /// <param name="message">The log message to display. Cannot be null.</param>
    void FlashLog(string message);

    /// <summary>
    /// Clears all log entries of the specified log type.
    /// </summary>
    /// <param name="logType">The type of log to clear. Specifies which category of log entries will be removed.</param>
    void ClearLog(LogType logType);

    /// <summary>
    /// Retrieves a list of log entries filtered by the specified log type.
    /// </summary>
    /// <param name="logType">The type of logs to retrieve. Determines which log entries are included in the returned list.</param>
    /// <returns>A list of strings containing the log entries that match the specified log type. The list is empty if no logs of
    /// the given type are available.</returns>
    List<string> GetLogs(LogType logType);
}