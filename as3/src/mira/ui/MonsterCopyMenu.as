package mira.ui {
  import flash.events.MouseEvent;
  import flash.system.ApplicationDomain;

  import mira.game.World;

  public class MonsterCopyMenu {
    private static var monsterClass:Class;

    public static function init(domain:ApplicationDomain):void {
      monsterClass = domain.getDefinition("MonsterMC") as Class;
    }

    public static function listenerFor(target:Object):Object {
      return target is monsterClass ? target : null;
    }

    public static function optionsAt(clip:Object, event:MouseEvent):Array {
      var monster:Object = clip.pAV;
      if (!monster || monster.npcType != "monster" || !monster.objData || !monster.dataLeaf) {
        return [];
      }

      var name:String = String(monster.objData.strMonName);
      return [
        CopyMenu.option("Copy Name", name),
        CopyMenu.option("Copy Monster ID", String(monster.objData.MonID)),
        CopyMenu.option("Copy Monster Map ID", String(monster.dataLeaf.MonMapID)),
        CopyMenu.option("Copy Monster", JSON.stringify(World.monsterRow(monster), null, 2), "monster " + name),
        CopyMenu.wiki(name)
      ];
    }
  }
}
