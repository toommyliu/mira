package mira.ui {
  import flash.events.MouseEvent;
  import flash.system.ApplicationDomain;

  import mira.Main;
  import mira.game.Shops;

  public class ShopCopyMenu {
    private static var panelBgClass:Class;
    private static var mergePanelBgClass:Class;
    private static var shopPanelClasses:Array;

    public static function init(domain:ApplicationDomain):void {
      panelBgClass = domain.getDefinition("LPFPanelBg") as Class;
      mergePanelBgClass = domain.getDefinition("LPFPanelBg2") as Class;
      shopPanelClasses = [
        domain.getDefinition("LPFPanelShopAQ"),
        domain.getDefinition("LPFPanelMergeShop"),
        domain.getDefinition("LPFPanelListShopInvA")
      ];
    }

    // Only a shop's title listens, so right-clicks anywhere else still reach Mira's own menu.
    // LPFPanelListShopInvA also shows house inventory, and the game titles it with the shop name only in a shop mode.
    public static function listenerFor(target:Object):Object {
      if (!(target is panelBgClass || target is mergePanelBgClass)) {
        return null;
      }

      var panel:Object = target.parent;
      var layout:Object = panel ? panel.parent : null;
      var inShopMode:Boolean = layout && "sMode" in layout && String(layout.sMode).toLowerCase().indexOf("shop") > -1;
      return inShopMode && isShopPanel(panel) ? target.tTitle : null;
    }

    private static function isShopPanel(panel:Object):Boolean {
      for each (var shopPanel:Class in shopPanelClasses) {
        if (panel is shopPanel) {
          return true;
        }
      }

      return false;
    }

    public static function optionsAt(title:Object, event:MouseEvent):Array {
      var shop:Object = Main.Game.world.shopinfo;
      if (!shop || title.text != shop.sName) {
        return [];
      }

      return [
        CopyMenu.option("Copy Name", String(shop.sName)),
        CopyMenu.option("Copy Shop ID", String(shop.ShopID)),
        CopyMenu.option("Copy Shop", JSON.stringify(Shops.getShop(), null, 2), "shop " + shop.sName),
        CopyMenu.wiki(shop.sName)
      ];
    }
  }
}
