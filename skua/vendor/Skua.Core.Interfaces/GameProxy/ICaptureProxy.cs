using System.ComponentModel;
using System.Net;

namespace Skua.Core.Interfaces;
/// <summary>
/// Defines the contract for a network capture proxy that relays traffic to a specified destination and supports packet
/// interception.
/// </summary>
/// <remarks>
/// Implementations of this interface allow inspection and modification of network traffic via registered
/// interceptors. The proxy can be started and stopped, and notifies listeners of property changes through the
/// <see cref="INotifyPropertyChanged"/> interface.
/// </remarks>
public interface ICaptureProxy : INotifyPropertyChanged
{
    /// <summary>
    /// The destination server for the proxy to relay traffic to and from.
    /// </summary>
    IPEndPoint? Destination { get; set; }

    /// <summary>
    /// The list of packet interceptors.
    /// </summary>
    List<IInterceptor> Interceptors { get; }

    /// <summary>
    /// Indicates whether the proxy is running or not.
    /// </summary>
    bool Running { get; }

    /// <summary>
    /// Starts the capture proxy.
    /// </summary>
    void Start();

    /// <summary>
    /// Stops the capture proxy.
    /// </summary>
    void Stop();
}