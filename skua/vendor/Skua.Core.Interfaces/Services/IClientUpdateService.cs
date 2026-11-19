using Skua.Core.Models.GitHub;

namespace Skua.Core.Interfaces;

/// <summary>
/// Defines the contract for a service that manages client update releases, including retrieving available updates and
/// downloading specific updates asynchronously.
/// </summary>
public interface IClientUpdateService
{
    /// <summary>
    /// Gets or sets the collection of available update releases.
    /// </summary>
    List<UpdateInfo> Releases { get; set; }

    /// <summary>
    /// Asynchronously retrieves release information from the data source.
    /// </summary>
    /// <returns>A task that represents the asynchronous operation.</returns>
    Task GetReleasesAsync();

    /// <summary>
    /// Asynchronously downloads the specified updates.
    /// </summary>
    /// <param name="progress">An optional progress reporter that receives status messages during the download operation. May be null if
    /// progress updates are not required.</param>
    /// <param name="info">The information describing the update to download. Cannot be null.</param>
    /// <returns>A task that represents the asynchronous download operation.</returns>
    Task DownloadUpdateAsync(IProgress<string>? progress, UpdateInfo info);
}