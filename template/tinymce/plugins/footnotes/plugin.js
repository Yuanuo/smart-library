(function(){
    'use strict';

    var global = tinymce.util.Tools.resolve('tinymce.PluginManager');

    if (typeof Array.prototype.forEach !== 'function') {
        Array.prototype.forEach = function(cb){
          for (var i = 0; i < this.length; i++){
            cb.apply(this, [this[i], i, this]);
          }
        };
    }
    if (Object.defineProperty 
        && Object.getOwnPropertyDescriptor 
        && Object.getOwnPropertyDescriptor(Element.prototype, "textContent") 
        && !Object.getOwnPropertyDescriptor(Element.prototype, "textContent").get) {
        (function() {
          var innerText = Object.getOwnPropertyDescriptor(Element.prototype, "innerText");
          Object.defineProperty(Element.prototype, "textContent",
           {
             get: function() {
               return innerText.get.call(this);
             },
             set: function(s) {
               return innerText.set.call(this, s);
             }
           }
         );
        })();
      }

    var open = function (editor) {
        const selectedNode = editor.selection.getNode();
        const selectedIsNote = selectedNode.tagName === 'A' && selectedNode.hasAttribute('data-note');
        const oldNote = selectedIsNote && decodeURIComponent(selectedNode.getAttribute('data-note')) || '';
        
        editor.windowManager.open({
            title: 'Text',
            size: 'normal',
            body: {
                type: 'panel',
                items : [
                    {
                        type:'textarea',
                        name: 'note',
                        multiline: true,
                        minWidth: 520,
                        minHeight: 100,
                    }
                ],
            },
            buttons: [
                {
                    type: 'cancel',
                    name: 'cancel',
                    text: 'Cancel'
                },
                {
                    type: 'submit',
                    name: 'save',
                    text: 'Save',
                    primary: true
                }
            ],

            initialData: { note: oldNote },
            onSubmit: function (e) {
                let uid = 'temp-' + new Date().getTime();
                let newNote = e.getData().note;
                let html = '<a id="' + uid + '" data-note="' + encodeURIComponent(newNote) + '"></a>';
                
                if (selectedIsNote){
                    editor.execCommand('mceReplaceContent', false, html);
                    editor.selection.collapse(0);
                }
                else {
                    editor.selection.collapse(0);
                    editor.execCommand('mceInsertContent', false, html);
                }

                e.close()
            }
        });
    };
    var Dialog = { open: open };
    var register$1 = function (editor) {
        editor.ui.registry.addToggleButton('footnotes', {
            icon : 'footnote',
            tooltip : 'Footnote',
            onAction: function () {
                return editor.execCommand('footnotes');
            },
            onSetup: function (buttonApi) {
                return editor.selection.selectorChangedWithUnbind('a[data-note]', buttonApi.setActive).unbind;
            }
        });
        editor.ui.registry.addMenuItem('footnotes', {
            icon: 'footnote',
            onAction: function () {
                return editor.execCommand('footnotes');
            }
        });
    };
    
    var register = function (editor) {
        editor.addCommand('footnotes', function () {
            Dialog.open(editor);
        });
    };

    var Commands = { register: register };
    var Buttons = { register: register$1 };

    function Plugin () {
        global.add('footnotes', function (editor) {
            editor.ui.registry.addIcon('footnote','<svg width="24px" height="24px"><path d="M8 21a1 1 0 0 1-1-1v-3H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2h-6.586l-3.707 3.707A1 1 0 0 1 8 21zM19 5H5v10h3a1 1 0 0 1 1 1v1.586l2.293-2.293A1 1 0 0 1 12 15h7V5z"/><circle cx="16" cy="10" r="1"/><circle cx="12" cy="10" r="1"/><circle cx="8" cy="10" r="1"/></svg>');
            Commands.register(editor);
            Buttons.register(editor);
        });
    }

    Plugin();
})()

