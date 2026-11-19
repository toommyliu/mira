using Skua.Core.Models.Skills;
using System.ComponentModel;

namespace Skua.Core.Interfaces;

/// <summary>
/// Represents a container for managing a collection of advanced skills, providing methods to load, modify, and persist
/// skill sets.
/// </summary>
/// <remarks>
/// Implementations of this interface notify listeners of property changes via the <see cref="INotifyPropertyChanged"/>
/// interface. Skill modifications made through this container are not automatically persisted; callers must explicitly
/// save changes to update the underlying storage.
/// </remarks>
public interface IAdvancedSkillContainer : INotifyPropertyChanged
{
    /// <summary>
    /// The current loaded sets of advanced skills.
    /// </summary>
    List<AdvancedSkill> LoadedSkills { get; set; }

    /// <summary>
    /// Adds an advanced skill to <see cref="LoadedSkills"/>
    /// </summary>
    /// <param name="skill">Skill to save.</param>
    /// <remarks>This doesn't save the modification to the file.</remarks>
    void Add(AdvancedSkill skill);

    /// <summary>
    /// Tries to replace a given skill, if not found, adds it to the list.
    /// </summary>
    /// <param name="skill">Skill to save.</param>
    void TryOverride(AdvancedSkill skill);

    /// <summary>
    /// Reads and loads all the skills from the skills file.
    /// </summary>
    void LoadSkills();

    /// <summary>
    /// Resets the skills to the default ones.
    /// </summary>
    void ResetSkillsSets();

    /// <summary>
    /// Saves the current skills to the skills file.
    /// </summary>
    void SyncSkills();

    /// <summary>
    /// Removes an <see cref="AdvancedSkill"/> from the <see cref="LoadedSkills"/>
    /// </summary>
    /// <param name="skill">Skill to remove.</param>
    /// <remarks>This doesn't save the modification to the file.</remarks>
    void Remove(AdvancedSkill skill);

    /// <summary>
    /// Saves all modifications to the skills file.
    /// </summary>
    void Save();

    /// <summary>
    /// Gets available class and mode combinations.
    /// </summary>
    Dictionary<string, List<string>> GetAvailableClassModes();

    /// <summary>
    /// Gets skills for a specific class and mode combination.
    /// </summary>
    AdvancedSkill? GetClassModeSkills(string className, string mode);
}
