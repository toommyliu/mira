namespace Skua.Core.Interfaces;


/// <summary>
/// Defines methods for creating required directories and files for client operations.
/// </summary>
/// <remarks>
/// Implementations of this interface are responsible for ensuring that the necessary directory structure
/// and files exist before performing file-related operations. The specific directories and files created may vary
/// depending on the implementation. Callers should consult the documentation of the implementing class for details
/// about the created resources and any associated side effects.
/// </remarks>
public interface IClientFilesService
{
    /// <summary>
    /// Creates all required directories for the current operation if they do not already exist.
    /// </summary>
    /// <remarks>
    /// This method ensures that any necessary directory structure is present before performing file
    /// operations. If the directories already exist, no action is taken.
    /// </remarks>
    void CreateDirectories();

    /// <summary>
    /// Creates one or more files as defined by the implementing class.
    /// </summary>
    /// <remarks>
    /// The specific files created and their locations depend on the implementation. Callers should
    /// refer to the documentation of the implementing class for details about which files are created and any side
    /// effects.
    /// </remarks>
    void CreateFiles();
}