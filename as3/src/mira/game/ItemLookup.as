package mira.game {
  public class ItemLookup {
    public static function toItemId(selector:Object):Number {
      if (!selector || !("itemId" in selector)) {
        return NaN;
      }

      var value:* = selector.itemId;
      if (value is int || value is uint || value is Number) {
        return Number(value);
      }

      return NaN;
    }

    public static function find(items:Array, selector:Object):Object {
      if (!selector || !(items is Array)) {
        return null;
      }

      var itemObj:Object;
      if ("name" in selector) {
        if (!(selector.name is String)) {
          return null;
        }

        var itemName:String = String(selector.name).toLowerCase();
        for each (itemObj in items) {
          if (String(itemObj.sName).toLowerCase() === itemName)
            return itemObj;
        }

        return null;
      }

      var itemId:Number = toItemId(selector);
      if (!isNaN(itemId)) {
        for each (itemObj in items) {
          if (Number(itemObj.ItemID) === itemId)
            return itemObj;
        }
      }

      return null;
    }
  }
}
