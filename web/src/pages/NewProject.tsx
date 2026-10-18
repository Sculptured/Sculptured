import { StartFromFile } from '../components/StartFromFile';
import { TemplateGallery } from '../structure/TemplateGallery';
import { useCreated } from './ProjectsHome';

/** `/new`: the template gallery as a page of its own: featured templates, More templates, Describe it and Advanced import. */
export function NewProject() {
  const created = useCreated();
  return (
    <main className="page page-wide new-project">
      <TemplateGallery inline flow={false} onCreated={created} advanced={<StartFromFile bare />} />
    </main>
  );
}
